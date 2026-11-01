//! The TypeCode constants SOMTC.DLL exports (`TC__long`, ...), laid out as
//! IBM's: a TypeCode is `{ kind, alignment (16 bits), 1, flags, the
//! kind's parameters... }`; the constants have no flags (not freed, no
//! explicit alignment). The descriptions are the interface repository's
//! (somir.idl's `Contained::Description` values).

#![allow(non_upper_case_globals)]

#[repr(C)]
pub struct W<const N: usize>(pub [*const u8; N]);
unsafe impl<const N: usize> Sync for W<N> {}

const fn n(x: usize) -> *const u8 {
    x as *const u8
}
const fn s(b: &'static [u8]) -> *const u8 {
    b.as_ptr()
}
const fn t<T>(r: &'static T) -> *const u8 {
    r as *const T as *const u8
}

/// Alignment 0, the always-set byte, no flags.
const F: *const u8 = n(0x10000);

macro_rules! basic {
    ($($name:ident = $k:expr;)*) => {$(
        #[unsafe(no_mangle)]
        pub static $name: W<2> = W([n($k), F]);
    )*};
}

basic! {
    TC__null = 1;
    TC__void = 2;
    TC__short = 3;
    TC__long = 4;
    TC__ushort = 5;
    TC__ulong = 6;
    TC__float = 7;
    TC__double = 8;
    TC__boolean = 9;
    TC__char = 10;
    TC__octet = 11;
    TC__any = 12;
    TC__TypeCode = 13;
    TC__Principal = 14;
}

#[unsafe(no_mangle)]
pub static TC__Object: W<3> = W([n(15), F, s(b"SOMObject\0")]);
#[unsafe(no_mangle)]
pub static TC__string: W<3> = W([n(19), F, n(0)]);

// The members of structures.
static STRING: W<3> = W([n(19), F, n(0)]);

static NV: W<8> = W([s(b"name\0"), t(&STRING), s(b"argument\0"), t(&TC__any), s(b"len\0"), t(&TC__long), s(b"arg_modes\0"), t(&TC__ulong)]);
#[unsafe(no_mangle)]
pub static TC__NamedValue: W<5> = W([n(16), F, n(4), s(b"NamedValue\0"), t(&NV)]);

/// name, id, defined_in: what every description starts with.
macro_rules! described {
    ($($more:expr),*) => {
        W([s(b"name\0"), t(&STRING), s(b"id\0"), t(&STRING), s(b"defined_in\0"), t(&STRING) $(, $more)*])
    };
}

static INTF: W<6> = described!();
#[unsafe(no_mangle)]
pub static TC__InterfaceDescription: W<5> = W([n(16), F, n(3), s(b"InterfaceDescription\0"), t(&INTF)]);

static MODULE: W<6> = described!();
#[unsafe(no_mangle)]
pub static TC__ModuleDescription: W<5> = W([n(16), F, n(3), s(b"ModuleDescription\0"), t(&MODULE)]);

static REPO: W<6> = described!();
#[unsafe(no_mangle)]
pub static TC__RepositoryDescription: W<5> = W([n(16), F, n(3), s(b"RepositoryDescription\0"), t(&REPO)]);

static CONST: W<10> = described!(s(b"type\0"), t(&TC__TypeCode), s(b"value\0"), t(&TC__any));
#[unsafe(no_mangle)]
pub static TC__ConstDescription: W<5> = W([n(16), F, n(5), s(b"ConstantDescription\0"), t(&CONST)]);

static TYPE: W<8> = described!(s(b"type\0"), t(&TC__TypeCode));
#[unsafe(no_mangle)]
pub static TC__TypeDescription: W<5> = W([n(16), F, n(4), s(b"TypeDescription\0"), t(&TYPE)]);

static EXCEPTION: W<8> = described!(s(b"type\0"), t(&TC__TypeCode));
#[unsafe(no_mangle)]
pub static TC__ExceptionDescription: W<5> = W([n(16), F, n(4), s(b"ExceptionDescription\0"), t(&EXCEPTION)]);

static PARAM_MODES: W<3> = W([s(b"IN\0"), s(b"OUT\0"), s(b"INOUT\0")]);
static PARAM_MODE: W<5> = W([n(18), F, n(3), s(b"ParameterMode\0"), t(&PARAM_MODES)]);
static PARAM: W<10> = described!(s(b"type\0"), t(&TC__TypeCode), s(b"mode\0"), t(&PARAM_MODE));
#[unsafe(no_mangle)]
pub static TC__ParameterDescription: W<5> = W([n(16), F, n(5), s(b"ParameterDescription\0"), t(&PARAM)]);

static ATTR_MODES: W<2> = W([s(b"NORMAL\0"), s(b"READONLY\0")]);
static ATTR_MODE: W<5> = W([n(18), F, n(2), s(b"AttributeMode\0"), t(&ATTR_MODES)]);
static ATTR: W<10> = described!(s(b"type\0"), t(&TC__TypeCode), s(b"mode\0"), t(&ATTR_MODE));
#[unsafe(no_mangle)]
pub static TC__AttributeDescription: W<5> = W([n(16), F, n(5), s(b"AttributeDescription\0"), t(&ATTR)]);

static OP_MODES: W<2> = W([s(b"NORMAL\0"), s(b"ONEWAY\0")]);
static OP_MODE: W<5> = W([n(18), F, n(2), s(b"OperationMode\0"), t(&OP_MODES)]);
static SEQ_STRING: W<4> = W([n(20), F, t(&STRING), n(0)]);
static SEQ_PARAM: W<4> = W([n(20), F, t(&TC__ParameterDescription), n(0)]);
static SEQ_EXCEPTION: W<4> = W([n(20), F, t(&TC__ExceptionDescription), n(0)]);
static OP: W<16> = described!(
    s(b"result\0"),
    t(&TC__TypeCode),
    s(b"mode\0"),
    t(&OP_MODE),
    s(b"contexts\0"),
    t(&SEQ_STRING),
    s(b"parameter\0"),
    t(&SEQ_PARAM),
    s(b"exceptions\0"),
    t(&SEQ_EXCEPTION)
);
#[unsafe(no_mangle)]
pub static TC__OperationDescription: W<5> = W([n(16), F, n(8), s(b"OperationDescription\0"), t(&OP)]);

static SEQ_OP: W<4> = W([n(20), F, t(&TC__OperationDescription), n(0)]);
static SEQ_ATTR: W<4> = W([n(20), F, t(&TC__AttributeDescription), n(0)]);
static FULL: W<10> = described!(s(b"operation\0"), t(&SEQ_OP), s(b"attribute\0"), t(&SEQ_ATTR));
#[unsafe(no_mangle)]
pub static TC__FullInterfaceDescription: W<5> = W([n(16), F, n(5), s(b"FullInterfaceDescription\0"), t(&FULL)]);

/// The value of a union's default label (tcParameter).
pub static ZERO_OCTET: u32 = 0;
