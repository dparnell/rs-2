//! OpenDoc's storage: ODStorageSystem, ODContainer, ODDocument, ODDraft,
//! ODStorageUnit (properties holding typed values), ODStorageUnitView and
//! ODStorageUnitCursor. Storage units live in memory; a draft is written
//! to and read from rs2's own container file (docs/OPENDOC.md):
//!
//! ```text
//! "RS2ODOC1", u32 units, per unit: u32 id, u32 properties,
//!   per property: u16 len + name, u32 values,
//!     per value: u16 len + type, u32 len + bytes
//! ```

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::obj::{acquire, release};
use crate::rt::*;
use crate::*;

#[derive(Clone, Default)]
pub struct Value {
    pub ty: Vec<u8>,
    pub data: Vec<u8>,
}

#[derive(Clone, Default)]
pub struct Prop {
    pub name: Vec<u8>,
    pub vals: Vec<Value>,
}

/// ODPositionCode.
pub mod pos {
    pub const UNDEFINED: usize = 0;
    pub const SAME: usize = 1;
    pub const ALL: usize = 2;
    pub const FIRST_SIB: usize = 3;
    pub const LAST_SIB: usize = 4;
    pub const NEXT_SIB: usize = 5;
    pub const PREV_SIB: usize = 6;
    pub const FIRST_BELOW: usize = 7;
    pub const LAST_BELOW: usize = 8;
    pub const FIRST_ABOVE: usize = 9;
    pub const LAST_ABOVE: usize = 10;
}

// ---- The container file ----

const MAGIC: &[u8; 8] = b"RS2ODOC1";

pub fn encode(units: &BTreeMap<u32, Vec<Prop>>) -> Vec<u8> {
    let mut o = Vec::new();
    o.extend_from_slice(MAGIC);
    o.extend_from_slice(&(units.len() as u32).to_le_bytes());
    for (id, props) in units {
        o.extend_from_slice(&id.to_le_bytes());
        o.extend_from_slice(&(props.len() as u32).to_le_bytes());
        for p in props {
            o.extend_from_slice(&(p.name.len() as u16).to_le_bytes());
            o.extend_from_slice(&p.name);
            o.extend_from_slice(&(p.vals.len() as u32).to_le_bytes());
            for v in &p.vals {
                o.extend_from_slice(&(v.ty.len() as u16).to_le_bytes());
                o.extend_from_slice(&v.ty);
                o.extend_from_slice(&(v.data.len() as u32).to_le_bytes());
                o.extend_from_slice(&v.data);
            }
        }
    }
    o
}

struct Rd<'a> {
    b: &'a [u8],
    i: usize,
}

impl Rd<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.b.get(self.i..self.i + n)?;
        self.i += n;
        Some(s)
    }
    fn u16(&mut self) -> Option<usize> {
        let s = self.take(2)?;
        Some(u16::from_le_bytes([s[0], s[1]]) as usize)
    }
    fn u32(&mut self) -> Option<u32> {
        let s = self.take(4)?;
        Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
}

pub fn decode(b: &[u8]) -> Option<BTreeMap<u32, Vec<Prop>>> {
    if b.get(..8)? != MAGIC {
        return None;
    }
    let mut r = Rd { b, i: 8 };
    let mut units = BTreeMap::new();
    for _ in 0..r.u32()? {
        let id = r.u32()?;
        let mut props = Vec::new();
        for _ in 0..r.u32()? {
            let n = r.u16()?;
            let name = r.take(n)?.to_vec();
            let mut vals = Vec::new();
            for _ in 0..r.u32()? {
                let n = r.u16()?;
                let ty = r.take(n)?.to_vec();
                let n = r.u32()? as usize;
                vals.push(Value { ty, data: r.take(n)?.to_vec() });
            }
            props.push(Prop { name, vals });
        }
        units.insert(id, props);
    }
    Some(units)
}

pub fn read_file(name: &[u8]) -> Option<Vec<u8>> {
    let n = z(name);
    let (mut hf, mut action) = (0u32, 0u32);
    // Open an existing file, read-only, deny write.
    if unsafe { os2::DosOpen(n.as_ptr(), &mut hf, &mut action, 0, 0, 0x01, 0x20, core::ptr::null_mut()) } != 0 {
        return None;
    }
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let mut got = 0u32;
        if unsafe { os2::DosRead(hf, buf.as_mut_ptr(), buf.len() as u32, &mut got) } != 0 || got == 0 {
            break;
        }
        out.extend_from_slice(&buf[..got as usize]);
    }
    unsafe { os2::DosClose(hf) };
    Some(out)
}

pub fn write_file(name: &[u8], data: &[u8]) -> bool {
    let n = z(name);
    let (mut hf, mut action) = (0u32, 0u32);
    // Create or replace, write-only, deny read/write.
    if unsafe { os2::DosOpen(n.as_ptr(), &mut hf, &mut action, 0, 0, 0x12, 0x11, core::ptr::null_mut()) } != 0 {
        return false;
    }
    let mut done = 0u32;
    let rc = unsafe { os2::DosWrite(hf, data.as_ptr(), data.len() as u32, &mut done) };
    unsafe { os2::DosClose(hf) };
    rc == 0 && done as usize == data.len()
}

// ---- ODStorageSystem ----

#[derive(Default)]
pub struct SystemState {
    pub session: usize,
    /// Open containers by file name.
    pub containers: Vec<(Vec<u8>, usize)>,
}

fn sys(this: usize) -> &'static mut SystemState {
    state(cd!(ODStorageSystemCClassData), this)
}

fn new_container(this: usize, ev: usize, id: &[u8], units: Option<BTreeMap<u32, Vec<Prop>>>) -> usize {
    let c = new_of(cd!(ODContainerClassData), "ODContainer");
    if c == 0 {
        return 0;
    }
    let cs = cont(c);
    cs.system = this;
    cs.file = id.to_vec();
    cs.exists = units.is_some();
    cs.units = units.unwrap_or_default();
    sys(this).containers.push((id.to_vec(), c));
    c
}

methods! {
    fn SysGetSession(this: usize, ev: usize) -> usize { sys(this).session }
    fn AcquireContainer(this: usize, ev: usize, ty: usize, id: usize) -> usize {
        let name = unsafe { ba_bytes(id) }.to_vec();
        let name: Vec<u8> = name.into_iter().take_while(|&b| b != 0).collect();
        if let Some(&(_, c)) = sys(this).containers.iter().find(|(n, _)| somsup::eq_nocase(n, &name)) {
            return acquire(c, ev);
        }
        match read_file(&name).as_deref().map(decode) {
            Some(Some(units)) => new_container(this, ev, &name, Some(units)),
            Some(None) => {
                od_error(ev, err::CANNOT_OPEN_CONTAINER);
                0
            }
            None => {
                od_error(ev, err::CONTAINER_DOES_NOT_EXIST);
                0
            }
        }
    }
    fn CreateContainer(this: usize, ev: usize, ty: usize, id: usize) -> usize {
        let name: Vec<u8> = unsafe { ba_bytes(id) }.iter().copied().take_while(|&b| b != 0).collect();
        new_container(this, ev, &name, None)
    }
    fn NeedSpace(this: usize, ev: usize, size: usize, do_purge: usize) -> usize { 0 }
    fn InitStorageSystem(this: usize, ev: usize, session: usize) -> usize {
        sys(this).session = session;
        0
    }
    fn ReleaseContainer(this: usize, ev: usize, c: usize) -> usize {
        sys(this).containers.retain(|&(_, x)| x != c);
        0
    }
    fn SysSomUninit(this: usize) -> usize {
        drop_state::<SystemState>(cd!(ODStorageSystemCClassData), this);
        call_parent(cd!(ODStorageSystemCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
}

pub static SYSTEM_M: &[(&str, M)] = &[
    ("GetSession", SysGetSession::t),
    ("AcquireContainer", AcquireContainer::t),
    ("CreateContainer", CreateContainer::t),
    ("NeedSpace", NeedSpace::t),
    ("InitStorageSystem", InitStorageSystem::t),
    ("ReleaseContainer", ReleaseContainer::t),
];
pub static SYSTEM_O: &[(&str, M)] = &[("SOMObject::somUninit", SysSomUninit::t)];

/// A new instance of one of this DLL's classes.
pub fn new_of(cd: *const usize, name: &str) -> usize {
    let cls = unsafe { *cd };
    let cls = if cls == 0 { crate::new_class(name) } else { cls };
    somsup::new_object(cls)
}

// ---- ODContainer ----

#[derive(Default)]
pub struct ContainerState {
    pub system: usize,
    pub file: Vec<u8>,
    pub exists: bool,
    /// The storage units as last read or written.
    pub units: BTreeMap<u32, Vec<Prop>>,
    pub document: usize,
}

pub fn cont(this: usize) -> &'static mut ContainerState {
    state(cd!(ODContainerCClassData), this)
}

methods! {
    fn ContGetStorageSystem(this: usize, ev: usize) -> usize { cont(this).system }
    fn ContGetID(this: usize, ev: usize) -> usize { 0 }
    fn ContGetName(this: usize, ev: usize, name: usize) -> usize { 0 }
    fn ContSetName(this: usize, ev: usize, name: usize) -> usize { 0 }
    fn AcquireDocument(this: usize, ev: usize, id: usize) -> usize {
        let c = cont(this);
        if c.document != 0 {
            return acquire(c.document, ev);
        }
        let d = new_of(cd!(ODDocumentClassData), "ODDocument");
        if d != 0 {
            doc(d).container = this;
            c.document = d;
            acquire(this, ev);
        }
        d
    }
    fn InitContainer(this: usize, ev: usize, system: usize, id: usize) -> usize {
        cont(this).system = system;
        0
    }
    fn ReleaseDocument(this: usize, ev: usize, d: usize) -> usize { 0 }
    fn ContRelease(this: usize, ev: usize) -> usize {
        let c = crate::obj::ref_count(this);
        if c <= 1 {
            let s = cont(this).system;
            if s != 0 {
                sys(s).containers.retain(|&(_, x)| x != this);
            }
        }
        call_parent(cd!(ODContainerCClassData), cd!(ODRefCntObjectClassData), crate::obj::ix::RELEASE, &[this, ev]);
        0
    }
    fn ContSomUninit(this: usize) -> usize {
        drop_state::<ContainerState>(cd!(ODContainerCClassData), this);
        call_parent(cd!(ODContainerCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
}

pub static CONTAINER_M: &[(&str, M)] = &[
    ("GetStorageSystem", ContGetStorageSystem::t),
    ("GetID", ContGetID::t),
    ("GetName", ContGetName::t),
    ("SetName", ContSetName::t),
    ("AcquireDocument", AcquireDocument::t),
    ("InitContainer", InitContainer::t),
    ("ReleaseDocument", ReleaseDocument::t),
];
pub static CONTAINER_O: &[(&str, M)] =
    &[("ODRefCntObject::Release", ContRelease::t), ("SOMObject::somUninit", ContSomUninit::t)];

// ---- ODDocument ----

#[derive(Default)]
pub struct DocumentState {
    pub container: usize,
    pub draft: usize,
    pub name: Vec<u8>,
}

pub fn doc(this: usize) -> &'static mut DocumentState {
    state(cd!(ODDocumentCClassData), this)
}

fn base_draft(this: usize, ev: usize) -> usize {
    let d = doc(this);
    if d.draft != 0 {
        return d.draft;
    }
    let dr = new_of(cd!(ODDraftClassData), "ODDraft");
    if dr == 0 {
        return 0;
    }
    let ds = draft(dr);
    ds.document = this;
    let c = cont(d.container);
    if c.exists {
        ds.stored = c.units.clone();
        ds.next_id = c.units.keys().max().copied().unwrap_or(0) + 1;
    } else {
        ds.next_id = 1;
        // Storage unit 1: the draft's properties.
        let su = new_su(dr, ev);
        release(su, ev);
    }
    d.draft = dr;
    dr
}

methods! {
    fn DocGetContainer(this: usize, ev: usize) -> usize { doc(this).container }
    fn DocGetID(this: usize, ev: usize) -> usize { 1 }
    fn DocGetName(this: usize, ev: usize) -> usize { dup(&doc(this).name) }
    fn DocSetName(this: usize, ev: usize, name: usize) -> usize {
        doc(this).name = cstr(name).to_vec();
        0
    }
    fn CollapseDrafts(this: usize, ev: usize, from: usize, to: usize) -> usize { this }
    fn AcquireDraft(this: usize, ev: usize, perms: usize, id: usize, d: usize, posc: usize, rel: usize) -> usize {
        acquire(base_draft(this, ev), ev)
    }
    fn DocExists(this: usize, ev: usize, id: usize, d: usize, posc: usize) -> usize { (id == 1 || d != 0) as usize }
    fn AcquireBaseDraft(this: usize, ev: usize, perms: usize) -> usize {
        let d = base_draft(this, ev);
        if d != 0 {
            draft(d).perms = perms as u32;
        }
        acquire(d, ev)
    }
    fn CreateDraft(this: usize, ev: usize, below: usize, release_below: usize) -> usize {
        acquire(base_draft(this, ev), ev)
    }
    fn SaveToAPrevious(this: usize, ev: usize, from: usize, to: usize) -> usize { 0 }
    fn SetBaseDraftFromForeignDraft(this: usize, ev: usize, d: usize) -> usize { 0 }
    fn InitDocument(this: usize, ev: usize, container: usize, id: usize) -> usize {
        doc(this).container = container;
        0
    }
    fn ReleaseDraft(this: usize, ev: usize, d: usize) -> usize { 0 }
    fn DocRelease(this: usize, ev: usize) -> usize {
        if crate::obj::ref_count(this) <= 1 {
            let d = doc(this);
            if d.draft != 0 {
                close_draft(d.draft, ev);
                som_free(d.draft);
                d.draft = 0;
            }
            let c = d.container;
            if c != 0 {
                cont(c).document = 0;
                d.container = 0;
                release(c, ev);
            }
        }
        call_parent(cd!(ODDocumentCClassData), cd!(ODRefCntObjectClassData), crate::obj::ix::RELEASE, &[this, ev]);
        0
    }
    fn DocSomUninit(this: usize) -> usize {
        drop_state::<DocumentState>(cd!(ODDocumentCClassData), this);
        call_parent(cd!(ODDocumentCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
}

pub static DOCUMENT_M: &[(&str, M)] = &[
    ("GetContainer", DocGetContainer::t),
    ("GetID", DocGetID::t),
    ("GetName", DocGetName::t),
    ("SetName", DocSetName::t),
    ("CollapseDrafts", CollapseDrafts::t),
    ("AcquireDraft", AcquireDraft::t),
    ("Exists", DocExists::t),
    ("AcquireBaseDraft", AcquireBaseDraft::t),
    ("CreateDraft", CreateDraft::t),
    ("SaveToAPrevious", SaveToAPrevious::t),
    ("SetBaseDraftFromForeignDraft", SetBaseDraftFromForeignDraft::t),
    ("InitDocument", InitDocument::t),
    ("ReleaseDraft", ReleaseDraft::t),
];
pub static DOCUMENT_O: &[(&str, M)] =
    &[("ODRefCntObject::Release", DocRelease::t), ("SOMObject::somUninit", DocSomUninit::t)];

// ---- ODDraft ----

#[derive(Default)]
pub struct DraftState {
    pub document: usize,
    /// Storage unit objects made in this session, by ID.
    pub sus: BTreeMap<u32, usize>,
    /// What the container held for the units not yet acquired.
    pub stored: BTreeMap<u32, Vec<Prop>>,
    pub next_id: u32,
    pub parts: BTreeMap<u32, usize>,
    pub frames: BTreeMap<u32, usize>,
    pub key: u32,
    pub perms: u32,
    pub changed: bool,
}

pub fn draft(this: usize) -> &'static mut DraftState {
    state(cd!(ODDraftCClassData), this)
}

pub mod draft_ix {
    pub const GET_DOCUMENT: usize = 0;
    pub const ACQUIRE_DRAFT_PROPERTIES: usize = 2;
    pub const EXTERNALIZE: usize = 4;
    pub const CREATE_STORAGE_UNIT: usize = 6;
    pub const ACQUIRE_STORAGE_UNIT: usize = 7;
    pub const CREATE_FRAME: usize = 20;
    pub const ACQUIRE_FRAME: usize = 21;
    pub const CREATE_PART: usize = 23;
    pub const ACQUIRE_PART: usize = 24;
    pub const RELEASE_PART: usize = 34;
    pub const RELEASE_FRAME: usize = 35;
}

fn new_su(dr: usize, ev: usize) -> usize {
    let d = draft(dr);
    let id = d.next_id;
    d.next_id += 1;
    let su = new_of(cd!(ODStorageUnitClassData), "ODStorageUnit");
    if su != 0 {
        let s = sus(su);
        s.id = id;
        s.draft = dr;
        // The draft's own reference keeps the data while the draft is open.
        acquire(su, ev);
        d.sus.insert(id, su);
        d.changed = true;
    }
    su
}

/// The storage unit `id` of a draft (acquired), from memory or the file.
pub fn draft_su(dr: usize, ev: usize, id: u32) -> usize {
    let d = draft(dr);
    if let Some(&su) = d.sus.get(&id) {
        return acquire(su, ev);
    }
    let Some(props) = d.stored.remove(&id) else {
        od_error(ev, err::INVALID_ID);
        return 0;
    };
    let su = new_of(cd!(ODStorageUnitClassData), "ODStorageUnit");
    if su != 0 {
        let s = sus(su);
        s.id = id;
        s.draft = dr;
        s.props = props;
        acquire(su, ev);
        d.sus.insert(id, su);
    }
    su
}

/// The units of a draft as they would be written.
pub fn draft_units(dr: usize) -> BTreeMap<u32, Vec<Prop>> {
    let d = draft(dr);
    let mut u = d.stored.clone();
    for (&id, &su) in &d.sus {
        u.insert(id, sus(su).props.clone());
    }
    u
}

/// Free everything a draft made (parts, frames, storage units).
pub fn close_draft(dr: usize, ev: usize) {
    let d = draft(dr);
    let parts: Vec<usize> = d.parts.values().copied().collect();
    d.parts.clear();
    for p in parts {
        call(cd!(ODPersistentObjectClassData), crate::obj::ix::RELEASE_ALL, &[p, ev]);
        som_free(p);
    }
    let frames: Vec<usize> = d.frames.values().copied().collect();
    d.frames.clear();
    for f in frames {
        crate::layout::frame_close(f);
        som_free(f);
    }
    let units: Vec<usize> = d.sus.values().copied().collect();
    d.sus.clear();
    for su in units {
        som_free(su);
    }
}

/// Find a part class by name (loading the DLL registered for it).
fn part_class(name: &[u8]) -> usize {
    let dll = crate::part::handler_dll(name);
    let mgr = unsafe { somsup::SOMClassMgrObject };
    let id = somsup::id(name) as usize;
    let cls = match &dll {
        Some(f) => {
            let f = z(f);
            call(somsup::scm(), somsup::mm::SOM_FIND_CLS_IN_FILE, &[mgr, id, 0, 0, f.as_ptr() as usize])
        }
        None => 0,
    };
    if cls != 0 { cls } else { call(somsup::scm(), somsup::mm::SOM_FIND_CLASS, &[mgr, id, 0, 0]) }
}

pub const PROP_EDITOR: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:Property:PreferredEditor";
pub const PROP_PART_KIND: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:Property:PreferredKind";
pub const PROP_OBJECT_TYPE: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:Property:ObjectType";
pub const TYPE_ISO_STR: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:Type:ISOStr";
pub const OBJ_PART: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:Object:Part";
pub const OBJ_FRAME: &[u8] = b"+//ISO 9070/ANSI::113722::US::CI LABS::OpenDoc:Object:Frame";

fn make_part(dr: usize, ev: usize, su: usize, cls_name: &[u8], from_storage: bool) -> usize {
    let cls = part_class(cls_name);
    if cls == 0 {
        od_error(ev, err::CANNOT_CREATE_PART);
        return 0;
    }
    let part = somsup::new_object(cls);
    if part == 0 {
        od_error(ev, err::CANNOT_CREATE_PART);
        return 0;
    }
    let id = su_id(su);
    draft(dr).parts.insert(id, part);
    // rs2 passes the part as its own part wrapper.
    let i = if from_storage { crate::part::ix::INIT_PART_FROM_STORAGE } else { crate::part::ix::INIT_PART };
    call(cd!(ODPartClassData), i, &[part, ev, su, part]);
    part
}

methods! {
    fn GetDocument(this: usize, ev: usize) -> usize { draft(this).document }
    fn DraftGetID(this: usize, ev: usize) -> usize { 1 }
    fn AcquireDraftProperties(this: usize, ev: usize) -> usize { draft_su(this, ev, 1) }
    fn GetPermissions(this: usize, ev: usize) -> usize { draft(this).perms as usize }
    fn DraftExternalize(this: usize, ev: usize) -> usize {
        let d = draft(this);
        let frames: Vec<usize> = d.frames.values().copied().collect();
        for f in frames {
            call(cd!(ODPersistentObjectClassData), crate::obj::ix::EXTERNALIZE, &[f, ev]);
        }
        let parts: Vec<usize> = draft(this).parts.values().copied().collect();
        for p in parts {
            call(cd!(ODPersistentObjectClassData), crate::obj::ix::EXTERNALIZE, &[p, ev]);
        }
        let units = draft_units(this);
        let c = doc(draft(this).document).container;
        let cs = cont(c);
        if write_file(&cs.file, &encode(&units)) {
            cs.units = units;
            cs.exists = true;
            draft(this).changed = false;
        } else {
            od_error(ev, err::CANNOT_OPEN_CONTAINER);
        }
        this
    }
    fn SaveToAPreviousDraft(this: usize, ev: usize, to: usize) -> usize { 0 }
    fn CreateStorageUnit(this: usize, ev: usize) -> usize { new_su(this, ev) }
    fn AcquireStorageUnit(this: usize, ev: usize, id: usize) -> usize { draft_su(this, ev, id as u32) }
    fn RemoveStorageUnit(this: usize, ev: usize, su: usize) -> usize {
        let id = su_id(su);
        draft(this).sus.remove(&id);
        draft(this).stored.remove(&id);
        0
    }
    fn BeginClone(this: usize, ev: usize, dest: usize, frame: usize, kind: usize) -> usize { new_clone_key() }
    fn EndClone(this: usize, ev: usize, key: usize) -> usize {
        end_clone(key);
        0
    }
    fn AbortClone(this: usize, ev: usize, key: usize) -> usize {
        end_clone(key);
        0
    }
    fn WeakClone(this: usize, ev: usize, key: usize, from: usize, to: usize, scope: usize) -> usize { to }
    fn IsValidID(this: usize, ev: usize, id: usize) -> usize {
        let d = draft(this);
        (d.sus.contains_key(&(id as u32)) || d.stored.contains_key(&(id as u32))) as usize
    }
    fn ChangedFromPrev(this: usize, ev: usize) -> usize { draft(this).changed as usize }
    fn SetChangedFromPrev(this: usize, ev: usize) -> usize {
        draft(this).changed = true;
        0
    }
    fn RemoveFromDocument(this: usize, ev: usize) -> usize { 0 }
    fn RemoveChanges(this: usize, ev: usize) -> usize { this }
    fn IsNewDraft(this: usize, ev: usize) -> usize { 0 }
    fn InitDraft(this: usize, ev: usize, document: usize, id: usize, perms: usize) -> usize {
        draft(this).document = document;
        draft(this).perms = perms as u32;
        0
    }
    fn CreateFrame(this: usize, ev: usize, frame_type: usize, containing: usize, shape: usize, bias: usize, part: usize,
                   view: usize, pres: usize, sub: usize, overlaid: usize) -> usize {
        let su = new_su(this, ev);
        let f = crate::layout::new_frame(this, ev, su, containing, shape, part, view, pres, sub & 0xff != 0, overlaid & 0xff != 0);
        release(su, ev);
        f
    }
    fn AcquireFrame(this: usize, ev: usize, id: usize) -> usize {
        if let Some(&f) = draft(this).frames.get(&(id as u32)) {
            return acquire(f, ev);
        }
        let su = draft_su(this, ev, id as u32);
        if su == 0 {
            return 0;
        }
        let f = crate::layout::frame_from_storage(this, ev, su);
        release(su, ev);
        f
    }
    fn RemoveFrame(this: usize, ev: usize, f: usize) -> usize {
        let id = crate::layout::frame_id(f);
        draft(this).frames.remove(&id);
        0
    }
    fn CreatePart(this: usize, ev: usize, kind: usize, editor: usize) -> usize {
        let cls_name: Vec<u8> = if editor != 0 && !cstr(editor).is_empty() {
            cstr(editor).to_vec()
        } else {
            match crate::part::handler_for_kind(cstr(kind)) {
                Some(c) => c,
                None => cstr(kind).to_vec(),
            }
        };
        let su = new_su(this, ev);
        set_value(su, PROP_OBJECT_TYPE, TYPE_ISO_STR, OBJ_PART);
        set_value(su, PROP_EDITOR, TYPE_ISO_STR, &cls_name);
        if kind != 0 {
            set_value(su, PROP_PART_KIND, TYPE_ISO_STR, cstr(kind));
        }
        let part = make_part(this, ev, su, &cls_name, false);
        release(su, ev);
        part
    }
    fn AcquirePart(this: usize, ev: usize, id: usize) -> usize {
        if let Some(&p) = draft(this).parts.get(&(id as u32)) {
            return acquire(p, ev);
        }
        let su = draft_su(this, ev, id as u32);
        if su == 0 {
            return 0;
        }
        let Some(editor) = get_value(su, PROP_EDITOR) else {
            release(su, ev);
            od_error(ev, err::CANNOT_CREATE_PART);
            return 0;
        };
        let part = make_part(this, ev, su, &editor, true);
        release(su, ev);
        part
    }
    fn RemovePart(this: usize, ev: usize, p: usize) -> usize {
        draft(this).parts.retain(|_, &mut x| x != p);
        0
    }
    fn CreateLinkSpec(this: usize, ev: usize, part: usize, data: usize) -> usize { 0 }
    fn CreateLinkSource(this: usize, ev: usize, part: usize) -> usize { 0 }
    fn AcquireLinkSource(this: usize, ev: usize, id: usize) -> usize { 0 }
    fn AcquireLink(this: usize, ev: usize, id: usize, spec: usize) -> usize { 0 }
    fn RemoveLink(this: usize, ev: usize, l: usize) -> usize { 0 }
    fn RemoveLinkSource(this: usize, ev: usize, l: usize) -> usize { 0 }
    fn GetPersistentObjectID(this: usize, ev: usize, obj: usize, ty: usize) -> usize {
        call(cd!(ODPersistentObjectClassData), crate::obj::ix::GET_ID, &[obj, ev])
    }
    fn AcquirePersistentObject(this: usize, ev: usize, id: usize, ty: usize) -> usize {
        let d = draft(this);
        if let Some(&p) = d.parts.get(&(id as u32)) {
            return acquire(p, ev);
        }
        if let Some(&f) = d.frames.get(&(id as u32)) {
            return acquire(f, ev);
        }
        0
    }
    fn ReleasePart(this: usize, ev: usize, part: usize) -> usize {
        // The draft keeps the part (unreferenced) until it is closed.
        0
    }
    fn ReleaseFrame(this: usize, ev: usize, frame: usize) -> usize { 0 }
    fn ReleaseLink(this: usize, ev: usize, l: usize) -> usize { 0 }
    fn ReleaseLinkSource(this: usize, ev: usize, l: usize) -> usize { 0 }
    fn ReleaseStorageUnit(this: usize, ev: usize, id: usize) -> usize { 0 }
    fn DraftRelease(this: usize, ev: usize) -> usize {
        // The document owns its draft; the count only goes down.
        let c: &mut i32 = somsup::data(cd!(ODRefCntObjectCClassData), this);
        if *c > 0 {
            *c -= 1;
        }
        0
    }
    fn DraftSomUninit(this: usize) -> usize {
        drop_state::<DraftState>(cd!(ODDraftCClassData), this);
        call_parent(cd!(ODDraftCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
}

pub static DRAFT_M: &[(&str, M)] = &[
    ("GetDocument", GetDocument::t),
    ("GetID", DraftGetID::t),
    ("AcquireDraftProperties", AcquireDraftProperties::t),
    ("GetPermissions", GetPermissions::t),
    ("Externalize", DraftExternalize::t),
    ("SaveToAPrevious", SaveToAPreviousDraft::t),
    ("CreateStorageUnit", CreateStorageUnit::t),
    ("AcquireStorageUnit", AcquireStorageUnit::t),
    ("RemoveStorageUnit", RemoveStorageUnit::t),
    ("BeginClone", BeginClone::t),
    ("EndClone", EndClone::t),
    ("AbortClone", AbortClone::t),
    ("WeakClone", WeakClone::t),
    ("IsValidID", IsValidID::t),
    ("ChangedFromPrev", ChangedFromPrev::t),
    ("SetChangedFromPrev", SetChangedFromPrev::t),
    ("RemoveFromDocument", RemoveFromDocument::t),
    ("RemoveChanges", RemoveChanges::t),
    ("IsNewDraft", IsNewDraft::t),
    ("InitDraft", InitDraft::t),
    ("CreateFrame", CreateFrame::t),
    ("AcquireFrame", AcquireFrame::t),
    ("RemoveFrame", RemoveFrame::t),
    ("CreatePart", CreatePart::t),
    ("AcquirePart", AcquirePart::t),
    ("RemovePart", RemovePart::t),
    ("CreateLinkSpec", CreateLinkSpec::t),
    ("CreateLinkSource", CreateLinkSource::t),
    ("AcquireLinkSource", AcquireLinkSource::t),
    ("AcquireLink", AcquireLink::t),
    ("RemoveLink", RemoveLink::t),
    ("RemoveLinkSource", RemoveLinkSource::t),
    ("GetPersistentObjectID", GetPersistentObjectID::t),
    ("AcquirePersistentObject", AcquirePersistentObject::t),
    ("ReleasePart", ReleasePart::t),
    ("ReleaseFrame", ReleaseFrame::t),
    ("ReleaseLink", ReleaseLink::t),
    ("ReleaseLinkSource", ReleaseLinkSource::t),
    ("ReleaseStorageUnit", ReleaseStorageUnit::t),
];
pub static DRAFT_O: &[(&str, M)] =
    &[("ODRefCntObject::Release", DraftRelease::t), ("SOMObject::somUninit", DraftSomUninit::t)];

// ---- ODStorageUnit ----

#[derive(Default)]
pub struct SuState {
    pub id: u32,
    pub draft: usize,
    pub props: Vec<Prop>,
    pub prop: Option<usize>,
    pub val: Option<usize>,
    pub offset: usize,
    pub name: Vec<u8>,
    pub generation: u32,
    pub lock: u32,
}

pub fn sus(this: usize) -> &'static mut SuState {
    state(cd!(ODStorageUnitCClassData), this)
}

pub fn su_id(su: usize) -> u32 {
    sus(su).id
}

/// Copy every property of one unit into another (CloneInto), and with
/// them the units its strong references name (copied into the
/// destination's draft once per clone operation `key`; weak references
/// keep their target when it was copied too, else go).
pub fn copy_su(from: usize, to: usize) {
    clone_into(from, to, 0);
}

/// Clone memos: per clone key, (source draft, source ID) -> new ID.
static mut MEMOS: Vec<(usize, BTreeMap<(usize, u32), u32>)> = Vec::new();
static mut NEXT_KEY: usize = 0;

#[allow(static_mut_refs)]
fn memo(key: usize) -> &'static mut BTreeMap<(usize, u32), u32> {
    unsafe {
        if let Some(i) = MEMOS.iter().position(|m| m.0 == key) {
            return &mut MEMOS[i].1;
        }
        MEMOS.push((key, BTreeMap::new()));
        &mut MEMOS.last_mut().unwrap().1
    }
}

pub fn new_clone_key() -> usize {
    unsafe {
        NEXT_KEY += 1;
        NEXT_KEY
    }
}

#[allow(static_mut_refs)]
pub fn end_clone(key: usize) {
    unsafe { MEMOS.retain(|m| m.0 != key) };
}

/// The copy of unit `id` of draft `src` in draft `dest` (made now).
fn clone_id(src: usize, id: u32, dest: usize, key: usize) -> u32 {
    if let Some(&n) = memo(key).get(&(src, id)) {
        return n;
    }
    let from = draft_su(src, 0, id);
    if from == 0 {
        return 0;
    }
    let to = new_su(dest, 0);
    let nid = su_id(to);
    memo(key).insert((src, id), nid);
    clone_into(from, to, key);
    release(to, 0);
    release(from, 0);
    nid
}

pub fn clone_into(from: usize, to: usize, key: usize) {
    let key = if key == 0 { new_clone_key() } else { key };
    let (src, dest) = (sus(from).draft, sus(to).draft);
    let props = sus(from).props.clone();
    let t = sus(to);
    for p in props {
        match t.props.iter_mut().find(|q| q.name == p.name) {
            Some(q) => q.vals = p.vals,
            None => t.props.push(p),
        }
    }
    // The reference table: each strong target copied into the destination.
    let old = refs(from);
    let mut new = Vec::with_capacity(old.len());
    for e in old {
        let (id, weak) = (e & !WEAK, e & WEAK != 0);
        let n = if id == 0 || src == 0 || dest == 0 {
            0
        } else if weak {
            memo(key).get(&(src, id)).copied().unwrap_or(if src == dest { id } else { 0 })
        } else {
            clone_id(src, id, dest, key)
        };
        new.push(if n == 0 { 0 } else { n | if weak { WEAK } else { 0 } });
    }
    set_refs(to, &new);
}

/// A unit's storage unit reference table (rs2: the property
/// `rs2:StorageUnitRefs`, one u32 per reference: the target's ID, the top
/// bit set for a weak reference). An ODStorageUnitRef is its index + 1.
pub const PROP_REFS: &[u8] = b"rs2:StorageUnitRefs";

pub fn refs(su: usize) -> Vec<u32> {
    let s = sus(su);
    let Some(p) = s.props.iter().find(|p| p.name == PROP_REFS) else { return Vec::new() };
    let Some(v) = p.vals.first() else { return Vec::new() };
    v.data.chunks(4).map(bytes_u32).collect()
}

pub fn set_refs_pub(su: usize, r: &[u32]) {
    set_refs(su, r)
}

fn set_refs(su: usize, r: &[u32]) {
    let s = sus(su);
    let mut d = Vec::new();
    for e in r {
        d.extend_from_slice(&e.to_le_bytes());
    }
    let v = Value { ty: b"rs2:Type:Refs".to_vec(), data: d };
    match s.props.iter_mut().find(|p| p.name == PROP_REFS) {
        Some(p) => p.vals = alloc::vec![v],
        None if r.is_empty() => {}
        None => s.props.push(Prop { name: PROP_REFS.to_vec(), vals: alloc::vec![v] }),
    }
}

/// The reference for (id, weak) in a unit's table (added if new).
fn make_ref(su: usize, id: u32, weak: bool) -> u32 {
    let e = id | if weak { WEAK } else { 0 };
    let mut r = refs(su);
    let i = match r.iter().position(|&x| x == e) {
        Some(i) => i,
        None => {
            r.push(e);
            set_refs(su, &r);
            r.len() - 1
        }
    };
    i as u32 + 1
}

/// The table entry a reference names.
fn ref_entry(su: usize, r: u32) -> Option<u32> {
    let i = r as usize;
    if i == 0 {
        return None;
    }
    refs(su).get(i - 1).copied().filter(|&e| e & !WEAK != 0)
}

/// The bytes of the first value of a property (rs2's own bookkeeping).
pub fn get_value(su: usize, prop: &[u8]) -> Option<Vec<u8>> {
    let s = sus(su);
    let p = s.props.iter().find(|p| p.name == prop)?;
    let v = p.vals.first()?;
    Some(v.data.iter().copied().take_while(|&b| b != 0).collect())
}

/// Set a property to a single value (a string gets its NUL).
pub fn set_value(su: usize, prop: &[u8], ty: &[u8], data: &[u8]) {
    let s = sus(su);
    let v = Value { ty: ty.to_vec(), data: z(data) };
    match s.props.iter_mut().find(|p| p.name == prop) {
        Some(p) => p.vals = alloc::vec![v],
        None => s.props.push(Prop { name: prop.to_vec(), vals: alloc::vec![v] }),
    }
}

impl SuState {
    fn cur_val(&mut self) -> Option<&mut Value> {
        let (p, v) = (self.prop?, self.val?);
        self.props.get_mut(p)?.vals.get_mut(v)
    }
    fn find_prop(&self, name: &[u8]) -> Option<usize> {
        self.props.iter().position(|p| p.name == name)
    }
}

/// Focus a unit (OpenDoc's Focus): a property by name or position, then a
/// value by type, index (1-based) or position.
fn focus(s: &mut SuState, prop: usize, ppos: usize, ty: usize, index: usize, vpos: usize) -> Result<(), i32> {
    let np = s.props.len();
    let p = if prop != 0 {
        s.find_prop(cstr(prop)).ok_or(err::PROPERTY_NOT_FOUND)?
    } else {
        match ppos {
            pos::FIRST_SIB if np > 0 => 0,
            pos::LAST_SIB if np > 0 => np - 1,
            pos::NEXT_SIB => match s.prop {
                Some(i) if i + 1 < np => i + 1,
                _ => return Err(err::PROPERTY_NOT_FOUND),
            },
            pos::PREV_SIB => match s.prop {
                Some(i) if i > 0 => i - 1,
                _ => return Err(err::PROPERTY_NOT_FOUND),
            },
            pos::UNDEFINED | pos::SAME | pos::ALL | pos::FIRST_ABOVE | pos::LAST_ABOVE => match s.prop {
                Some(i) => i,
                None if ppos == pos::UNDEFINED || ppos == pos::ALL || ppos >= pos::FIRST_ABOVE => {
                    // Focus on the whole unit.
                    s.prop = None;
                    s.val = None;
                    s.offset = 0;
                    return Ok(());
                }
                None => return Err(err::UNFOCUSED_STORAGE_UNIT),
            },
            _ => return Err(err::PROPERTY_NOT_FOUND),
        }
    };
    let vals = &s.props[p].vals;
    let nv = vals.len();
    let v = if ty != 0 {
        Some(vals.iter().position(|v| v.ty == cstr(ty)).ok_or(err::VALUE_NOT_FOUND)?)
    } else if index != 0 {
        if index > nv {
            return Err(err::VALUE_INDEX_OUT_OF_RANGE);
        }
        Some(index - 1)
    } else {
        match vpos {
            pos::FIRST_SIB | pos::FIRST_BELOW if nv > 0 => Some(0),
            pos::LAST_SIB | pos::LAST_BELOW if nv > 0 => Some(nv - 1),
            pos::NEXT_SIB => match s.val {
                Some(i) if s.prop == Some(p) && i + 1 < nv => Some(i + 1),
                _ => return Err(err::VALUE_NOT_FOUND),
            },
            pos::PREV_SIB => match s.val {
                Some(i) if s.prop == Some(p) && i > 0 => Some(i - 1),
                _ => return Err(err::VALUE_NOT_FOUND),
            },
            pos::SAME if s.prop == Some(p) => s.val,
            _ => None,
        }
    };
    s.prop = Some(p);
    s.val = v;
    s.offset = 0;
    Ok(())
}

/// ODStorageUnitRef: the ID of the unit referred to (weak: top bit set).
const WEAK: u32 = 0x8000_0000;

fn draft_has(dr: usize, id: u32) -> bool {
    let d = draft(dr);
    d.sus.contains_key(&id) || d.stored.contains_key(&id)
}

pub mod su_ix {
    pub const GET_DRAFT: usize = 1;
    pub const EXISTS: usize = 3;
    pub const FOCUS: usize = 7;
    pub const GET_ID: usize = 11;
    pub const ADD_PROPERTY: usize = 14;
    pub const ADD_VALUE: usize = 15;
    pub const REMOVE: usize = 16;
    pub const GET_VALUE: usize = 26;
    pub const SET_VALUE: usize = 27;
    pub const GET_SIZE: usize = 30;
    pub const GET_STRONG_REF: usize = 32;
    pub const GET_WEAK_REF: usize = 33;
    pub const GET_ID_FROM_REF: usize = 37;
}

methods! {
    fn InitStorageUnit(this: usize, ev: usize, d: usize, id: usize) -> usize {
        let s = sus(this);
        s.draft = d;
        s.id = id as u32;
        0
    }
    fn SuGetDraft(this: usize, ev: usize) -> usize { sus(this).draft }
    fn SuGetSession(this: usize, ev: usize) -> usize { crate::services::session() }
    fn Exists(this: usize, ev: usize, prop: usize, ty: usize, index: usize) -> usize {
        let s = sus(this);
        let p = if prop != 0 { s.find_prop(cstr(prop)) } else { s.prop };
        let Some(p) = p else { return 0 };
        let vals = &s.props[p].vals;
        if ty != 0 {
            vals.iter().any(|v| v.ty == cstr(ty)) as usize
        } else if index != 0 {
            (index <= vals.len()) as usize
        } else {
            1
        }
    }
    fn ExistsWithCursor(this: usize, ev: usize, cursor: usize) -> usize {
        let c = cursor_state(cursor);
        let (p, t) = (z(&c.prop), z(&c.ty));
        Exists(this, ev, if c.prop.is_empty() { 0 } else { p.as_ptr() as usize }, if c.ty.is_empty() { 0 } else { t.as_ptr() as usize }, c.index as usize)
    }
    fn CountProperties(this: usize, ev: usize) -> usize { sus(this).props.len() }
    fn CountValues(this: usize, ev: usize) -> usize {
        let s = sus(this);
        s.prop.map_or(0, |p| s.props[p].vals.len())
    }
    fn Focus(this: usize, ev: usize, prop: usize, ppos: usize, ty: usize, index: usize, vpos: usize) -> usize {
        if let Err(e) = focus(sus(this), prop, ppos, ty, index, vpos) {
            od_error(ev, e);
        }
        this
    }
    fn FocusWithCursor(this: usize, ev: usize, cursor: usize) -> usize {
        let c = cursor_state(cursor);
        let (p, t) = (z(&c.prop), z(&c.ty));
        Focus(this, ev, if c.prop.is_empty() { 0 } else { p.as_ptr() as usize }, pos::UNDEFINED,
              if c.ty.is_empty() { 0 } else { t.as_ptr() as usize }, c.index as usize, pos::UNDEFINED)
    }
    fn SuExternalize(this: usize, ev: usize) -> usize { this }
    fn SuInternalize(this: usize, ev: usize) -> usize { this }
    fn SuGetID(this: usize, ev: usize) -> usize { sus(this).id as usize }
    fn SuGetName(this: usize, ev: usize) -> usize { dup(&sus(this).name) }
    fn SuSetName(this: usize, ev: usize, name: usize) -> usize {
        sus(this).name = cstr(name).to_vec();
        0
    }
    fn AddProperty(this: usize, ev: usize, name: usize) -> usize {
        let s = sus(this);
        let n = cstr(name);
        let p = match s.find_prop(n) {
            Some(p) => p,
            None => {
                s.props.push(Prop { name: n.to_vec(), vals: Vec::new() });
                s.props.len() - 1
            }
        };
        s.prop = Some(p);
        s.val = None;
        s.offset = 0;
        this
    }
    fn AddValue(this: usize, ev: usize, ty: usize) -> usize {
        let s = sus(this);
        let Some(p) = s.prop else {
            od_error(ev, err::UNFOCUSED_STORAGE_UNIT);
            return this;
        };
        let t = cstr(ty);
        let vals = &mut s.props[p].vals;
        let v = match vals.iter().position(|v| v.ty == t) {
            Some(v) => v,
            None => {
                vals.push(Value { ty: t.to_vec(), data: Vec::new() });
                vals.len() - 1
            }
        };
        s.val = Some(v);
        s.offset = 0;
        this
    }
    fn Remove(this: usize, ev: usize) -> usize {
        let s = sus(this);
        match (s.prop, s.val) {
            (Some(p), Some(v)) => {
                s.props[p].vals.remove(v);
                s.val = None;
            }
            (Some(p), None) => {
                s.props.remove(p);
                s.prop = None;
            }
            _ => s.props.clear(),
        }
        s.offset = 0;
        this
    }
    fn SuCloneInto(this: usize, ev: usize, key: usize, dest: usize, scope: usize) -> usize {
        if dest != 0 {
            clone_into(this, dest, key);
        }
        0
    }
    fn CreateView(this: usize, ev: usize) -> usize {
        let v = new_of(cd!(ODStorageUnitViewClassData), "ODStorageUnitView");
        if v != 0 {
            let s = sus(this);
            let vs = view(v);
            vs.su = acquire(this, ev);
            vs.prop = s.prop.map(|p| s.props[p].name.clone()).unwrap_or_default();
            vs.ty = match (s.prop, s.val) {
                (Some(p), Some(i)) => s.props[p].vals[i].ty.clone(),
                _ => Vec::new(),
            };
        }
        v
    }
    fn CreateCursor(this: usize, ev: usize, prop: usize, ty: usize, index: usize) -> usize {
        let c = new_of(cd!(ODStorageUnitCursorClassData), "ODStorageUnitCursor");
        if c != 0 {
            let cs = cursor_state(c);
            cs.prop = cstr(prop).to_vec();
            cs.ty = cstr(ty).to_vec();
            cs.index = index as u32;
        }
        c
    }
    fn CreateCursorWithFocus(this: usize, ev: usize) -> usize {
        let s = sus(this);
        let p = s.prop.map(|p| z(&s.props[p].name)).unwrap_or_else(|| alloc::vec![0]);
        let t = match (s.prop, s.val) {
            (Some(p), Some(i)) => z(&s.props[p].vals[i].ty),
            _ => alloc::vec![0],
        };
        CreateCursor(this, ev, p.as_ptr() as usize, t.as_ptr() as usize, 0)
    }
    fn GetCurrentProperty(this: usize, ev: usize) -> usize {
        let s = sus(this);
        match s.prop {
            Some(p) => dup(&s.props[p].name),
            None => {
                od_error(ev, err::UNFOCUSED_STORAGE_UNIT);
                0
            }
        }
    }
    fn GetCurrentValueType(this: usize, ev: usize) -> usize {
        let s = sus(this);
        match (s.prop, s.val) {
            (Some(p), Some(v)) => dup(&s.props[p].vals[v].ty),
            _ => {
                od_error(ev, err::UNFOCUSED_STORAGE_UNIT);
                0
            }
        }
    }
    fn SetType(this: usize, ev: usize, ty: usize) -> usize {
        match sus(this).cur_val() {
            Some(v) => v.ty = cstr(ty).to_vec(),
            None => od_error(ev, err::UNFOCUSED_STORAGE_UNIT),
        }
        0
    }
    fn SetOffset(this: usize, ev: usize, off: usize) -> usize {
        sus(this).offset = off;
        0
    }
    fn GetOffset(this: usize, ev: usize) -> usize { sus(this).offset }
    fn GetValue(this: usize, ev: usize, len: usize, ba: usize) -> usize {
        let s = sus(this);
        let off = s.offset;
        let Some(v) = s.cur_val() else {
            od_error(ev, err::UNFOCUSED_STORAGE_UNIT);
            ba_fill(ba, &[]);
            return 0;
        };
        let end = (off + len).min(v.data.len());
        let bytes = if off < end { v.data[off..end].to_vec() } else { Vec::new() };
        ba_fill(ba, &bytes);
        s.offset = off + bytes.len();
        bytes.len()
    }
    fn SetValue(this: usize, ev: usize, ba: usize) -> usize {
        let data = unsafe { ba_bytes(ba) }.to_vec();
        let s = sus(this);
        let off = s.offset;
        let Some(v) = s.cur_val() else {
            od_error(ev, err::UNFOCUSED_STORAGE_UNIT);
            return 0;
        };
        if v.data.len() < off + data.len() {
            v.data.resize(off + data.len(), 0);
        }
        v.data[off..off + data.len()].copy_from_slice(&data);
        s.offset = off + data.len();
        0
    }
    fn InsertValue(this: usize, ev: usize, ba: usize) -> usize {
        let data = unsafe { ba_bytes(ba) }.to_vec();
        let s = sus(this);
        let off = s.offset;
        let Some(v) = s.cur_val() else {
            od_error(ev, err::UNFOCUSED_STORAGE_UNIT);
            return 0;
        };
        let at = off.min(v.data.len());
        let tail = v.data.split_off(at);
        v.data.extend_from_slice(&data);
        v.data.extend_from_slice(&tail);
        s.offset = at + data.len();
        0
    }
    fn DeleteValue(this: usize, ev: usize, len: usize) -> usize {
        let s = sus(this);
        let off = s.offset;
        if let Some(v) = s.cur_val() {
            let end = (off + len).min(v.data.len());
            if off < end {
                v.data.drain(off..end);
            }
        }
        0
    }
    fn GetSize(this: usize, ev: usize) -> usize {
        let s = sus(this);
        match (s.prop, s.val) {
            (Some(p), Some(v)) => s.props[p].vals[v].data.len(),
            (Some(p), None) => s.props[p].vals.iter().map(|v| v.data.len()).sum(),
            _ => s.props.iter().flat_map(|p| p.vals.iter()).map(|v| v.data.len()).sum(),
        }
    }
    fn IsValidStorageUnitRef(this: usize, ev: usize, r: usize) -> usize {
        let id = if r == 0 { 0 } else { ref_entry(this, read::<u32>(r)).map_or(0, |e| e & !WEAK) };
        (id != 0 && draft_has(sus(this).draft, id)) as usize
    }
    fn GetStrongStorageUnitRef(this: usize, ev: usize, id: usize, r: usize) -> usize {
        write::<u32>(r, make_ref(this, id as u32, false));
        0
    }
    fn GetWeakStorageUnitRef(this: usize, ev: usize, id: usize, r: usize) -> usize {
        write::<u32>(r, make_ref(this, id as u32, true));
        0
    }
    fn IsStrongStorageUnitRef(this: usize, ev: usize, r: usize) -> usize {
        (r != 0 && ref_entry(this, read::<u32>(r)).is_some_and(|e| e & WEAK == 0)) as usize
    }
    fn IsWeakStorageUnitRef(this: usize, ev: usize, r: usize) -> usize {
        (r != 0 && ref_entry(this, read::<u32>(r)).is_some_and(|e| e & WEAK != 0)) as usize
    }
    fn RemoveStorageUnitRef(this: usize, ev: usize, r: usize) -> usize {
        if r != 0 {
            let i = read::<u32>(r) as usize;
            let mut t = refs(this);
            if i >= 1 && i <= t.len() {
                t[i - 1] = 0;
                set_refs(this, &t);
            }
        }
        this
    }
    fn GetIDFromStorageUnitRef(this: usize, ev: usize, r: usize) -> usize {
        let id = if r == 0 { 0 } else { ref_entry(this, read::<u32>(r)).map_or(0, |e| e & !WEAK) };
        if id == 0 || !draft_has(sus(this).draft, id) {
            od_error(ev, err::INVALID_STORAGE_UNIT_REF);
            return 0;
        }
        id as usize
    }
    fn CreateStorageUnitRefIterator(this: usize, ev: usize) -> usize { 0 }
    fn GetGenerationNumber(this: usize, ev: usize) -> usize { sus(this).generation as usize }
    fn IncrementGenerationNumber(this: usize, ev: usize) -> usize {
        sus(this).generation += 1;
        sus(this).generation as usize
    }
    fn IsPromiseValue(this: usize, ev: usize) -> usize { 0 }
    fn ClearAllPromises(this: usize, ev: usize) -> usize { 0 }
    fn SetPromiseValue(this: usize, ev: usize, ty: usize, off: usize, ba: usize, part: usize) -> usize {
        // Promises are kept as their data.
        let s = sus(this);
        if let Some(p) = s.prop {
            let t = cstr(ty).to_vec();
            let vals = &mut s.props[p].vals;
            let i = match vals.iter().position(|v| v.ty == t) {
                Some(i) => i,
                None => {
                    vals.push(Value { ty: t, data: Vec::new() });
                    vals.len() - 1
                }
            };
            s.val = Some(i);
            s.offset = off;
            SetValue(this, ev, ba);
        }
        0
    }
    fn GetPromiseValue(this: usize, ev: usize, ty: usize, off: usize, len: usize, ba: usize, part: usize) -> usize {
        write::<usize>(part, 0);
        SetOffset(this, ev, off);
        GetValue(this, ev, len, ba)
    }
    fn ResolveAllPromises(this: usize, ev: usize) -> usize { 0 }
    fn GetHeap(this: usize, ev: usize) -> usize { 0 }
    fn Lock(this: usize, ev: usize, key: usize) -> usize {
        let s = sus(this);
        s.lock += 1;
        s.lock as usize
    }
    fn Unlock(this: usize, ev: usize, key: usize) -> usize {
        let s = sus(this);
        s.lock = s.lock.saturating_sub(1);
        0
    }
    fn SuRelease(this: usize, ev: usize) -> usize {
        // The draft's own reference keeps the unit until the draft closes.
        let c: &mut i32 = somsup::data(cd!(ODRefCntObjectCClassData), this);
        *c -= 1;
        if *c <= 0 {
            som_free(this);
        }
        0
    }
    fn SuSomUninit(this: usize) -> usize {
        drop_state::<SuState>(cd!(ODStorageUnitCClassData), this);
        call_parent(cd!(ODStorageUnitCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
}

pub static SU_M: &[(&str, M)] = &[
    ("InitStorageUnit", InitStorageUnit::t),
    ("GetDraft", SuGetDraft::t),
    ("GetSession", SuGetSession::t),
    ("Exists", Exists::t),
    ("ExistsWithCursor", ExistsWithCursor::t),
    ("CountProperties", CountProperties::t),
    ("CountValues", CountValues::t),
    ("Focus", Focus::t),
    ("FocusWithCursor", FocusWithCursor::t),
    ("Externalize", SuExternalize::t),
    ("Internalize", SuInternalize::t),
    ("GetID", SuGetID::t),
    ("GetName", SuGetName::t),
    ("SetName", SuSetName::t),
    ("AddProperty", AddProperty::t),
    ("AddValue", AddValue::t),
    ("Remove", Remove::t),
    ("CloneInto", SuCloneInto::t),
    ("CreateView", CreateView::t),
    ("CreateCursor", CreateCursor::t),
    ("CreateCursorWithFocus", CreateCursorWithFocus::t),
    ("GetCurrentProperty", GetCurrentProperty::t),
    ("GetCurrentValueType", GetCurrentValueType::t),
    ("SetType", SetType::t),
    ("SetOffset", SetOffset::t),
    ("GetOffset", GetOffset::t),
    ("GetValue", GetValue::t),
    ("SetValue", SetValue::t),
    ("InsertValue", InsertValue::t),
    ("DeleteValue", DeleteValue::t),
    ("GetSize", GetSize::t),
    ("IsValidStorageUnitRef", IsValidStorageUnitRef::t),
    ("GetStrongStorageUnitRef", GetStrongStorageUnitRef::t),
    ("GetWeakStorageUnitRef", GetWeakStorageUnitRef::t),
    ("IsStrongStorageUnitRef", IsStrongStorageUnitRef::t),
    ("IsWeakStorageUnitRef", IsWeakStorageUnitRef::t),
    ("RemoveStorageUnitRef", RemoveStorageUnitRef::t),
    ("GetIDFromStorageUnitRef", GetIDFromStorageUnitRef::t),
    ("CreateStorageUnitRefIterator", CreateStorageUnitRefIterator::t),
    ("GetGenerationNumber", GetGenerationNumber::t),
    ("IncrementGenerationNumber", IncrementGenerationNumber::t),
    ("IsPromiseValue", IsPromiseValue::t),
    ("ClearAllPromises", ClearAllPromises::t),
    ("SetPromiseValue", SetPromiseValue::t),
    ("GetPromiseValue", GetPromiseValue::t),
    ("ResolveAllPromises", ResolveAllPromises::t),
    ("GetHeap", GetHeap::t),
    ("Lock", Lock::t),
    ("Unlock", Unlock::t),
];
pub static SU_O: &[(&str, M)] = &[("ODRefCntObject::Release", SuRelease::t), ("SOMObject::somUninit", SuSomUninit::t)];

/// StorageUnitGetValue(su, ev, size, buffer) (ordinal 810): copy up to
/// `size` bytes of the focused value to `buffer`; the bytes copied.
pub extern "C" fn storage_unit_get_value(su: usize, ev: usize, size: usize, buf: usize) -> usize {
    let mut ba = ByteArray { maximum: 0, length: 0, buffer: core::ptr::null_mut() };
    let n = call(cd!(ODStorageUnitClassData), su_ix::GET_VALUE, &[su, ev, size, &raw mut ba as usize]);
    if !ba.buffer.is_null() {
        if buf != 0 {
            unsafe { core::ptr::copy_nonoverlapping(ba.buffer, buf as *mut u8, (ba.length as usize).min(size)) };
        }
        somsup::free(ba.buffer);
    }
    n
}

// ---- ODStorageUnitView: a unit and a focus ----

#[derive(Default)]
pub struct ViewState {
    pub su: usize,
    pub prop: Vec<u8>,
    pub ty: Vec<u8>,
    pub offset: usize,
}

pub fn view(this: usize) -> &'static mut ViewState {
    state(cd!(ODStorageUnitViewCClassData), this)
}

/// Focus the view's unit as the view is, then call a unit method.
fn via(this: usize, ev: usize, i: usize, rest: &[usize]) -> usize {
    let v = view(this);
    let s = sus(v.su);
    if let Some(p) = s.find_prop(&v.prop) {
        s.prop = Some(p);
        s.val = s.props[p].vals.iter().position(|x| x.ty == v.ty);
    }
    s.offset = v.offset;
    let mut args = alloc::vec![v.su, ev];
    args.extend_from_slice(rest);
    let r = call(cd!(ODStorageUnitClassData), i, &args);
    view(this).offset = sus(view(this).su).offset;
    r
}

methods! {
    fn InitStorageUnitView(this: usize, ev: usize, su: usize, cursor: usize) -> usize {
        let v = view(this);
        v.su = acquire(su, ev);
        if cursor != 0 {
            let c = cursor_state(cursor);
            v.prop = c.prop.clone();
            v.ty = c.ty.clone();
        }
        0
    }
    fn VGetStorageUnit(this: usize, ev: usize) -> usize { view(this).su }
    fn VGetCursor(this: usize, ev: usize) -> usize {
        let v = view(this);
        let (p, t) = (z(&v.prop), z(&v.ty));
        CreateCursor(v.su, ev, p.as_ptr() as usize, t.as_ptr() as usize, 0)
    }
    fn VExternalize(this: usize, ev: usize) -> usize { this }
    fn VInternalize(this: usize, ev: usize) -> usize { this }
    fn VGetID(this: usize, ev: usize) -> usize { sus(view(this).su).id as usize }
    fn VGetName(this: usize, ev: usize) -> usize { SuGetName(view(this).su, ev) }
    fn VSetName(this: usize, ev: usize, n: usize) -> usize { SuSetName(view(this).su, ev, n) }
    fn VAddProperty(this: usize, ev: usize, n: usize) -> usize {
        view(this).prop = cstr(n).to_vec();
        via(this, ev, su_ix::ADD_PROPERTY, &[n]);
        this
    }
    fn VAddValue(this: usize, ev: usize, t: usize) -> usize {
        view(this).ty = cstr(t).to_vec();
        via(this, ev, su_ix::ADD_VALUE, &[t]);
        this
    }
    fn VRemove(this: usize, ev: usize) -> usize {
        via(this, ev, su_ix::REMOVE, &[]);
        this
    }
    fn VCloneInto(this: usize, ev: usize, key: usize, dest: usize, scope: usize) -> usize { SuCloneInto(view(this).su, ev, key, dest, scope) }
    fn VGetProperty(this: usize, ev: usize) -> usize { dup(&view(this).prop) }
    fn VGetType(this: usize, ev: usize) -> usize { dup(&view(this).ty) }
    fn VSetType(this: usize, ev: usize, t: usize) -> usize {
        via(this, ev, 23, &[t]);
        view(this).ty = cstr(t).to_vec();
        0
    }
    fn VSetOffset(this: usize, ev: usize, off: usize) -> usize {
        view(this).offset = off;
        0
    }
    fn VGetOffset(this: usize, ev: usize) -> usize { view(this).offset }
    fn VGetValue(this: usize, ev: usize, len: usize, ba: usize) -> usize { via(this, ev, su_ix::GET_VALUE, &[len, ba]) }
    fn VSetValue(this: usize, ev: usize, ba: usize) -> usize { via(this, ev, su_ix::SET_VALUE, &[ba]) }
    fn VInsertValue(this: usize, ev: usize, ba: usize) -> usize { via(this, ev, 28, &[ba]) }
    fn VDeleteValue(this: usize, ev: usize, len: usize) -> usize { via(this, ev, 29, &[len]) }
    fn VGetSize(this: usize, ev: usize) -> usize { via(this, ev, su_ix::GET_SIZE, &[]) }
    fn VIsValidStorageUnitRef(this: usize, ev: usize, r: usize) -> usize { IsValidStorageUnitRef(view(this).su, ev, r) }
    fn VGetStrongStorageUnitRef(this: usize, ev: usize, id: usize, r: usize) -> usize { GetStrongStorageUnitRef(view(this).su, ev, id, r) }
    fn VGetWeakStorageUnitRef(this: usize, ev: usize, id: usize, r: usize) -> usize { GetWeakStorageUnitRef(view(this).su, ev, id, r) }
    fn VIsStrongStorageUnitRef(this: usize, ev: usize, r: usize) -> usize { IsStrongStorageUnitRef(view(this).su, ev, r) }
    fn VIsWeakStorageUnitRef(this: usize, ev: usize, r: usize) -> usize { IsWeakStorageUnitRef(view(this).su, ev, r) }
    fn VRemoveStorageUnitRef(this: usize, ev: usize, r: usize) -> usize { this }
    fn VGetIDFromStorageUnitRef(this: usize, ev: usize, r: usize) -> usize { GetIDFromStorageUnitRef(view(this).su, ev, r) }
    fn VCreateStorageUnitRefIterator(this: usize, ev: usize) -> usize { 0 }
    fn VGetGenerationNumber(this: usize, ev: usize) -> usize { GetGenerationNumber(view(this).su, ev) }
    fn VIncrementGenerationNumber(this: usize, ev: usize) -> usize { IncrementGenerationNumber(view(this).su, ev) }
    fn VIsPromiseValue(this: usize, ev: usize) -> usize { 0 }
    fn VSetPromiseValue(this: usize, ev: usize, ty: usize, off: usize, ba: usize, part: usize) -> usize {
        via(this, ev, 43, &[ty, off, ba, part])
    }
    fn VGetPromiseValue(this: usize, ev: usize, ty: usize, off: usize, len: usize, ba: usize, part: usize) -> usize {
        via(this, ev, 44, &[ty, off, len, ba, part])
    }
    fn VSomUninit(this: usize) -> usize {
        let su = view(this).su;
        drop_state::<ViewState>(cd!(ODStorageUnitViewCClassData), this);
        release(su, 0);
        call_parent(cd!(ODStorageUnitViewCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
}

pub static VIEW_M: &[(&str, M)] = &[
    ("InitStorageUnitView", InitStorageUnitView::t),
    ("GetStorageUnit", VGetStorageUnit::t),
    ("GetCursor", VGetCursor::t),
    ("Externalize", VExternalize::t),
    ("Internalize", VInternalize::t),
    ("GetID", VGetID::t),
    ("GetName", VGetName::t),
    ("SetName", VSetName::t),
    ("AddProperty", VAddProperty::t),
    ("AddValue", VAddValue::t),
    ("Remove", VRemove::t),
    ("CloneInto", VCloneInto::t),
    ("GetProperty", VGetProperty::t),
    ("GetType", VGetType::t),
    ("SetType", VSetType::t),
    ("SetOffset", VSetOffset::t),
    ("GetOffset", VGetOffset::t),
    ("GetValue", VGetValue::t),
    ("SetValue", VSetValue::t),
    ("InsertValue", VInsertValue::t),
    ("DeleteValue", VDeleteValue::t),
    ("GetSize", VGetSize::t),
    ("IsValidStorageUnitRef", VIsValidStorageUnitRef::t),
    ("GetStrongStorageUnitRef", VGetStrongStorageUnitRef::t),
    ("GetWeakStorageUnitRef", VGetWeakStorageUnitRef::t),
    ("IsStrongStorageUnitRef", VIsStrongStorageUnitRef::t),
    ("IsWeakStorageUnitRef", VIsWeakStorageUnitRef::t),
    ("RemoveStorageUnitRef", VRemoveStorageUnitRef::t),
    ("GetIDFromStorageUnitRef", VGetIDFromStorageUnitRef::t),
    ("CreateStorageUnitRefIterator", VCreateStorageUnitRefIterator::t),
    ("GetGenerationNumber", VGetGenerationNumber::t),
    ("IncrementGenerationNumber", VIncrementGenerationNumber::t),
    ("IsPromiseValue", VIsPromiseValue::t),
    ("SetPromiseValue", VSetPromiseValue::t),
    ("GetPromiseValue", VGetPromiseValue::t),
];
pub static VIEW_O: &[(&str, M)] = &[("SOMObject::somUninit", VSomUninit::t)];

// ---- ODStorageUnitCursor ----

#[derive(Default)]
pub struct CursorState {
    pub prop: Vec<u8>,
    pub ty: Vec<u8>,
    pub index: u32,
}

pub fn cursor_state(this: usize) -> &'static mut CursorState {
    state(cd!(ODStorageUnitCursorCClassData), this)
}

methods! {
    fn InitStorageUnitCursor(this: usize, ev: usize, prop: usize, ty: usize, index: usize) -> usize {
        let c = cursor_state(this);
        c.prop = cstr(prop).to_vec();
        c.ty = cstr(ty).to_vec();
        c.index = index as u32;
        0
    }
    fn CGetProperty(this: usize, ev: usize, out: usize) -> usize {
        write::<usize>(out, dup(&cursor_state(this).prop));
        0
    }
    fn CGetValueType(this: usize, ev: usize, out: usize) -> usize {
        write::<usize>(out, dup(&cursor_state(this).ty));
        0
    }
    fn CGetValueIndex(this: usize, ev: usize, out: usize) -> usize {
        write::<u32>(out, cursor_state(this).index);
        0
    }
    fn CSetProperty(this: usize, ev: usize, p: usize) -> usize {
        cursor_state(this).prop = cstr(p).to_vec();
        0
    }
    fn CSetValueType(this: usize, ev: usize, t: usize) -> usize {
        cursor_state(this).ty = cstr(t).to_vec();
        0
    }
    fn CSetValueIndex(this: usize, ev: usize, i: usize) -> usize {
        cursor_state(this).index = i as u32;
        0
    }
    fn CSomUninit(this: usize) -> usize {
        drop_state::<CursorState>(cd!(ODStorageUnitCursorCClassData), this);
        call_parent(cd!(ODStorageUnitCursorCClassData), somsup::so(), somsup::om::SOM_UNINIT, &[this]);
        0
    }
}

pub static CURSOR_M: &[(&str, M)] = &[
    ("InitStorageUnitCursor", InitStorageUnitCursor::t),
    ("GetProperty", CGetProperty::t),
    ("GetValueType", CGetValueType::t),
    ("GetValueIndex", CGetValueIndex::t),
    ("SetProperty", CSetProperty::t),
    ("SetValueType", CSetValueType::t),
    ("SetValueIndex", CSetValueIndex::t),
];
pub static CURSOR_O: &[(&str, M)] = &[("SOMObject::somUninit", CSomUninit::t)];
