//! Object REXX for programs that hold REXX objects themselves: OREXX.DLL's
//! OSA support calls (RexxCreateMethod, RexxRunMethod...) and its
//! native-method API (REXX_SEND, REXX_STRING_NEW...), which IBM's OpenDoc
//! scripting component OREXXSC.DLL uses.

use alloc::rc::Rc;
use alloc::vec::Vec;

use crate::exec::{Frame, Interp};
use crate::parse::Program;
use crate::value::{Builtin, Data, Value};
use crate::{CallType, Err};

/// Check a program (directives included) without running it: the REXX
/// error number (major * 1000 + minor for Object REXX's) and line.
pub fn check_program(src: &[u8], name: &[u8]) -> Result<(), (u32, u32)> {
    match crate::oo::load_package(src, name) {
        Ok(_) => Ok(()),
        Err((e, line)) => Err((
            match e {
                Err::Syntax(c) => c,
                _ => 49,
            },
            line,
        )),
    }
}

impl<'h> Interp<'h> {
    /// The objects of an Object REXX interpreter.
    pub fn world(&self) -> Option<Rc<crate::oo::World>> {
        self.w.clone()
    }

    fn top_frame(&self) -> Frame {
        Frame::new(Rc::new(Program::empty()), b"", b"CMD", CallType::Function, self.st.pools.pools.len())
    }

    /// The program and its activities have ended: UNINIT for the objects
    /// still alive that have one.
    pub fn end_uninits(&mut self) {
        if self.w.is_some() {
            let mut f = self.top_frame();
            (self.ops().run_uninits)(self, &mut f, true);
        }
    }

    /// Send a message from outside any REXX code. Errors are the REXX
    /// error number.
    pub fn send_value(&mut self, recv: &Value, name: &[u8], args: Vec<Option<Value>>) -> Result<Option<Value>, u32> {
        let mut f = self.top_frame();
        let mut n = name.to_vec();
        n.make_ascii_uppercase();
        match self.send(&mut f, recv, &n, args, crate::oo::Start::Normal, 0) {
            Ok(v) => Ok(v),
            Err(Err::Syntax(c)) | Err(Err::Fatal(c)) => Err(c),
            Err(_) => Err(40),
        }
    }
}

impl crate::oo::World {
    pub fn nil_value(&self) -> Value {
        Value::O(self.nil.clone())
    }
    pub fn environment(&self) -> Value {
        Value::O(self.env.clone())
    }
    pub fn local_dir(&self) -> Value {
        Value::O(self.local.clone())
    }
    pub fn new_directory(&self) -> Value {
        self.obj(Builtin::Directory, Data::Directory(Default::default()))
    }
    /// An array's items (None for other objects).
    pub fn array_items(v: &Value) -> Option<Vec<Option<Value>>> {
        match v {
            Value::O(o) => match &*o.data.borrow() {
                Data::Array(a) => Some(a.items.clone()),
                _ => None,
            },
            _ => None,
        }
    }
    /// Put an item into an array (index from 1; the array grows).
    pub fn array_put(v: &Value, item: Option<Value>, index: usize) -> bool {
        if let Value::O(o) = v {
            if let Data::Array(a) = &mut *o.data.borrow_mut() {
                if index == 0 {
                    return false;
                }
                if a.items.len() < index {
                    a.items.resize(index, None);
                }
                a.items[index - 1] = item;
                return true;
            }
        }
        false
    }
    pub fn is_directory(v: &Value) -> bool {
        matches!(v, Value::O(o) if matches!(&*o.data.borrow(), Data::Directory(_)))
    }
    pub fn is_string(v: &Value) -> bool {
        match v {
            Value::S(_) => true,
            Value::O(o) => matches!(&*o.data.borrow(), Data::Str(_)),
        }
    }
}
