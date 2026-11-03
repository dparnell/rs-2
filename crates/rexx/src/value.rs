//! Values and objects. Classic REXX has only strings; Object REXX values
//! are strings or references to objects. A string value is `Value::S`
//! (an instance of .String without an object of its own); everything
//! else is an [`Object`] behind an `Rc`.

use alloc::collections::{BTreeMap, VecDeque};
use alloc::rc::{Rc, Weak};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};

use crate::parse::Program;
use crate::vars::{PoolRef, StemRef};

pub type ObjRef = Rc<Object>;

#[derive(Clone)]
pub enum Value {
    S(Vec<u8>),
    O(ObjRef),
}

impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Value {
        Value::S(v)
    }
}

impl From<&[u8]> for Value {
    fn from(v: &[u8]) -> Value {
        Value::S(v.to_vec())
    }
}

impl Value {
    pub fn str(&self) -> Option<&[u8]> {
        match self {
            Value::S(s) => Some(s),
            Value::O(_) => None,
        }
    }
    pub fn obj(&self) -> Option<&ObjRef> {
        match self {
            Value::O(o) => Some(o),
            Value::S(_) => None,
        }
    }
    /// The string of a value without running any REXX method: strings
    /// themselves, a stem's default value (or its name), a class's id,
    /// else the object's name.
    pub fn fast_string(&self) -> Vec<u8> {
        match self {
            Value::S(s) => s.clone(),
            Value::O(o) => o.fast_string(),
        }
    }
    /// Identity: the same object, or equal strings.
    pub fn same(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::S(a), Value::S(b)) => a == b,
            (Value::O(a), Value::O(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
    /// The key of a value in a collection (strings by value, objects by
    /// identity).
    pub fn key(&self) -> Key {
        match self {
            Value::S(s) => Key::S(s.clone()),
            Value::O(o) => match &*o.data.borrow() {
                Data::Str(s) => Key::S(s.clone()),
                _ => Key::O(o.id),
            },
        }
    }
}

/// A collection key.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Key {
    S(Vec<u8>),
    O(u64),
}

/// The built-in classes (and what their instances hold).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Builtin {
    Object,
    Class,
    String,
    Array,
    List,
    Queue,
    Directory,
    Table,
    Relation,
    Set,
    Bag,
    Stem,
    Supplier,
    Message,
    Method,
    Monitor,
    Stream,
    Alarm,
}

pub struct Object {
    pub id: u64,
    /// The class (a class object). Set once the bootstrap is done.
    pub class: RefCell<Option<ObjRef>>,
    pub data: RefCell<Data>,
    /// Object variable pools, one per scope (class id * 2 + class side).
    pub vars: RefCell<Vec<(u64, PoolRef)>>,
    /// Methods of the object itself (SETMETHOD, ENHANCED).
    pub methods: RefCell<Option<BTreeMap<Vec<u8>, Method>>>,
    /// OBJECTNAME=.
    pub name: RefCell<Option<Vec<u8>>>,
    /// Exclusive use of the variable pool (GUARD): the activity holding it
    /// and how often.
    pub guard: Cell<(u64, u32)>,
    /// Objects with an UNINIT method: where the object goes when its last
    /// reference is dropped (the world's queue, World::uninit_queue), to
    /// be sent UNINIT at the next clause.
    pub uninit: RefCell<Option<UninitQueue>>,
}

/// Objects waiting for their UNINIT method.
pub type UninitQueue = Rc<RefCell<Vec<Object>>>;

impl Drop for Object {
    fn drop(&mut self) {
        let Some(q) = self.uninit.get_mut().take() else { return };
        // The last reference is gone: the object lives on in the queue
        // (with the same identity) until UNINIT has run.
        let o = Object {
            id: self.id,
            class: RefCell::new(self.class.get_mut().take()),
            data: RefCell::new(core::mem::replace(self.data.get_mut(), Data::Plain)),
            vars: RefCell::new(core::mem::take(self.vars.get_mut())),
            methods: RefCell::new(self.methods.get_mut().take()),
            name: RefCell::new(self.name.get_mut().take()),
            guard: Cell::new((0, 0)),
            uninit: RefCell::new(None),
        };
        if let Ok(mut v) = q.try_borrow_mut() {
            v.push(o);
        }
    }
}

impl Object {
    pub fn class(&self) -> ObjRef {
        self.class.borrow().clone().expect("object without a class")
    }
    pub fn is_class(&self) -> bool {
        matches!(&*self.data.borrow(), Data::Class(_))
    }
    /// The class's id, for class objects.
    pub fn class_id(&self) -> Vec<u8> {
        match &*self.data.borrow() {
            Data::Class(c) => c.id.clone(),
            _ => Vec::new(),
        }
    }
    pub fn fast_string(&self) -> Vec<u8> {
        if let Some(n) = &*self.name.borrow() {
            return n.clone();
        }
        match &*self.data.borrow() {
            Data::Str(s) => return s.clone(),
            Data::Nil => return b"The NIL object".to_vec(),
            Data::Class(_) => {}
            Data::Stream(s) => return s.name.clone(),
            Data::Stem(s, name) => {
                return match &s.borrow().default {
                    Some(v) => v.fast_string(),
                    None => name.clone(),
                };
            }
            _ => {}
        }
        self.default_name()
    }
    /// "a CLASS" / "an Array".
    pub fn default_name(&self) -> Vec<u8> {
        let id = match &*self.data.borrow() {
            Data::Class(c) => {
                // a class's default name is "The X class"
                let mut v = b"The ".to_vec();
                v.extend_from_slice(&c.id);
                v.extend_from_slice(b" class");
                return v;
            }
            _ => self.class.borrow().as_ref().map(|c| c.class_id()).unwrap_or_default(),
        };
        article(&id)
    }
}

/// "a NAME" or "an NAME".
pub fn article(id: &[u8]) -> Vec<u8> {
    let vowel = id.first().is_some_and(|c| b"AEIOUaeiou".contains(c));
    let mut v = if vowel { b"an ".to_vec() } else { b"a ".to_vec() };
    v.extend_from_slice(id);
    v
}

pub enum Data {
    Plain,
    Nil,
    /// An instance of a subclass of .String.
    Str(Vec<u8>),
    Class(alloc::boxed::Box<ClassInfo>),
    Array(ArrayData),
    List(ListData),
    Queue(VecDeque<Value>),
    Directory(DirData),
    /// Table, relation, set and bag: items by index key.
    Coll(BTreeMap<Key, Vec<(Value, Value)>>),
    Stem(StemRef, Vec<u8>),
    Supplier(SupplierData),
    Message(alloc::boxed::Box<MsgData>),
    Method(Method),
    /// The destinations, the current one last (DESTINATION pushes and pops).
    Monitor(Vec<Value>),
    Stream(StreamData),
    Alarm(AlarmData),
    /// An object of another object system (SOM, the Workplace Shell):
    /// (kind, handle) for the host.
    Ext(u32, usize),
}

#[derive(Default, Clone)]
pub struct ArrayData {
    pub items: Vec<Option<Value>>,
    /// Dimensions of a multi-dimensional array (empty: one dimension).
    pub dims: Vec<usize>,
}

#[derive(Default)]
pub struct ListData {
    pub items: VecDeque<(u64, Value)>,
    pub next: u64,
}

#[derive(Default)]
pub struct DirData {
    pub map: BTreeMap<Vec<u8>, Value>,
    /// SETENTRY/SETMETHOD methods: run when the index is looked up.
    pub methods: BTreeMap<Vec<u8>, Method>,
    pub unknown: Option<Method>,
}

pub struct SupplierData {
    pub items: Vec<Value>,
    pub indexes: Vec<Value>,
    pub pos: usize,
}

pub struct MsgData {
    pub target: Value,
    pub name: Vec<u8>,
    pub scope: Option<ObjRef>,
    pub args: Vec<Option<Value>>,
    /// 0 not sent, 1 running, 2 done, 3 failed.
    pub state: u8,
    pub result: Option<Value>,
    pub notify: Vec<Value>,
}

#[derive(Default)]
pub struct StreamData {
    pub name: Vec<u8>,
}

pub struct AlarmData {
    pub msg: Value,
    pub cancelled: bool,
}

/// What a method runs.
#[derive(Clone)]
pub enum Body {
    Rexx(Rc<Code>),
    /// An attribute's getter or setter (the variable name).
    Get(Vec<u8>),
    Set(Vec<u8>),
    /// A method of a built-in class (looked up by name).
    Native(Builtin),
    /// A method removed by DEFINE with no source (hides inherited ones).
    Null,
    /// `::METHOD name EXTERNAL 'LIBRARY dll [entry]'`: a native method the
    /// host calls (Host::native_method).
    External(Vec<u8>),
}

#[derive(Clone)]
pub struct Method {
    pub body: Body,
    pub private: bool,
    pub guarded: bool,
    pub protected: bool,
}

impl Method {
    pub fn rexx(code: Rc<Code>) -> Method {
        Method { body: Body::Rexx(code), private: false, guarded: true, protected: false }
    }
}

/// REXX code of a method or routine.
pub struct Code {
    pub prog: Rc<Program>,
    pub pkg: Rc<Package>,
    pub name: Vec<u8>,
    /// The source lines (Method~source).
    pub source: Vec<Vec<u8>>,
}

pub struct ClassInfo {
    pub id: Vec<u8>,
    /// Instance methods defined by this class.
    pub inst: BTreeMap<Vec<u8>, Method>,
    /// Class methods defined by this class (inherited by subclasses).
    pub cls: BTreeMap<Vec<u8>, Method>,
    /// The superclass, then inherited mixins.
    pub supers: Vec<ObjRef>,
    pub subs: Vec<Weak<Object>>,
    /// The metaclass whose instances are this class's subclasses' class
    /// objects (None: the class's own class).
    pub meta: Option<ObjRef>,
    pub builtin: Option<Builtin>,
    pub mixin: bool,
    /// External classes: (kind, handle) for the host (SOM classes).
    pub ext: Option<(u32, usize)>,
    pub pkg: Option<Rc<Package>>,
}

impl ClassInfo {
    pub fn new(id: Vec<u8>) -> ClassInfo {
        ClassInfo {
            id,
            inst: BTreeMap::new(),
            cls: BTreeMap::new(),
            supers: Vec::new(),
            subs: Vec::new(),
            meta: None,
            builtin: None,
            mixin: false,
            ext: None,
            pkg: None,
        }
    }
}

/// A directive in the source: ::METHOD.
pub struct MethodDir {
    pub name: Vec<u8>,
    pub class_side: bool,
    pub attribute: bool,
    pub private: bool,
    pub guarded: bool,
    pub protected: bool,
    pub external: Option<Vec<u8>>,
    pub prog: Rc<Program>,
    pub source: Vec<Vec<u8>>,
    pub line: u32,
}

/// ::CLASS.
pub struct ClassDir {
    pub name: Vec<u8>,
    pub id: Vec<u8>,
    pub public: bool,
    pub metaclass: Option<Vec<u8>>,
    pub subclass: Option<Vec<u8>>,
    pub mixin: bool,
    pub inherit: Vec<Vec<u8>>,
    pub external: Option<Vec<u8>>,
    pub methods: Vec<MethodDir>,
    pub line: u32,
}

pub struct RoutineDir {
    pub prog: Rc<Program>,
    pub public: bool,
    pub source: Vec<Vec<u8>>,
}

/// A program file (or INTERPRETed/method source) with its directives.
pub struct Package {
    pub name: Vec<u8>,
    pub main: Rc<Program>,
    pub routines: BTreeMap<Vec<u8>, RoutineDir>,
    pub classes: Vec<ClassDir>,
    /// ::METHODs before the first ::CLASS (the .METHODS directory).
    pub floating: Vec<MethodDir>,
    pub requires: Vec<(Vec<u8>, u32)>,
    /// Filled in when the package is installed.
    pub installed: RefCell<Installed>,
}

#[derive(Default)]
pub struct Installed {
    pub done: bool,
    /// This package's classes by name.
    pub classes: BTreeMap<Vec<u8>, ObjRef>,
    /// Packages this one requires.
    pub imports: Vec<Rc<Package>>,
    pub methods: Option<ObjRef>,
}
