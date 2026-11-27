//! <math.h> on the x87 unit (core has no transcendental functions):
//! values are computed in 80-bit precision and rounded to double.

use core::arch::asm;

use crate::{set_errno, EDOM, ERANGE};

macro_rules! x87 {
    ($x:expr, $($ins:expr),+) => {{
        let mut v: f64 = $x;
        unsafe {
            asm!(
                "fld qword ptr [{p}]",
                $($ins,)+
                "fstp qword ptr [{p}]",
                p = in(reg) &mut v as *mut f64,
                out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
                out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
                out("eax") _,
                options(nostack)
            );
        }
        v
    }};
}

/// 2^st(0), for any st(0).
macro_rules! exp2_tail {
    () => {
        concat!(
            "fld st(0)\n",
            "frndint\n",
            "fsub st(1), st(0)\n",
            "fxch st(1)\n",
            "f2xm1\n",
            "fld1\n",
            "faddp st(1), st(0)\n",
            "fscale\n",
            "fstp st(1)\n"
        )
    };
}

pub fn sqrt_f(x: f64) -> f64 {
    if x < 0.0 {
        set_errno(EDOM);
        return f64::NAN;
    }
    x87!(x, "fsqrt")
}

#[unsafe(no_mangle)]
pub extern "C" fn sqrt(x: f64) -> f64 {
    sqrt_f(x)
}

/// Reduce a large argument for fsin/fcos/fptan (they want |x| < 2^63).
fn reduce(x: f64) -> f64 {
    if x.abs() < 1e18 { x } else { fmod_f(x, core::f64::consts::TAU) }
}

#[unsafe(no_mangle)]
pub extern "C" fn sin(x: f64) -> f64 {
    if x.is_infinite() {
        set_errno(EDOM);
        return f64::NAN;
    }
    x87!(reduce(x), "fsin")
}

#[unsafe(no_mangle)]
pub extern "C" fn cos(x: f64) -> f64 {
    if x.is_infinite() {
        set_errno(EDOM);
        return f64::NAN;
    }
    x87!(reduce(x), "fcos")
}

#[unsafe(no_mangle)]
pub extern "C" fn tan(x: f64) -> f64 {
    if x.is_infinite() {
        set_errno(EDOM);
        return f64::NAN;
    }
    x87!(reduce(x), "fptan", "fstp st(0)")
}

#[unsafe(no_mangle)]
pub extern "C" fn atan(x: f64) -> f64 {
    x87!(x, "fld1", "fpatan")
}

pub fn atan2_f(y: f64, x: f64) -> f64 {
    let mut v = y;
    let xx = x;
    unsafe {
        asm!(
            "fld qword ptr [{p}]",
            "fld qword ptr [{q}]",
            "fpatan",
            "fstp qword ptr [{p}]",
            p = in(reg) &mut v as *mut f64,
            q = in(reg) &xx as *const f64,
            out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
            out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            options(nostack)
        );
    }
    v
}

#[unsafe(no_mangle)]
pub extern "C" fn atan2(y: f64, x: f64) -> f64 {
    atan2_f(y, x)
}

#[unsafe(no_mangle)]
pub extern "C" fn asin(x: f64) -> f64 {
    if !(-1.0..=1.0).contains(&x) {
        set_errno(EDOM);
        return f64::NAN;
    }
    atan2_f(x, sqrt_f((1.0 - x) * (1.0 + x)))
}

#[unsafe(no_mangle)]
pub extern "C" fn acos(x: f64) -> f64 {
    if !(-1.0..=1.0).contains(&x) {
        set_errno(EDOM);
        return f64::NAN;
    }
    atan2_f(sqrt_f((1.0 - x) * (1.0 + x)), x)
}

pub fn exp_f(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x == f64::INFINITY {
        return x;
    }
    if x == f64::NEG_INFINITY {
        return 0.0;
    }
    let r = x87!(x, "fldl2e", "fmulp st(1), st(0)", exp2_tail!());
    if r.is_infinite() {
        set_errno(ERANGE);
    }
    r
}

#[unsafe(no_mangle)]
pub extern "C" fn exp(x: f64) -> f64 {
    exp_f(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn exp2(x: f64) -> f64 {
    if x.is_infinite() {
        return if x > 0.0 { x } else { 0.0 };
    }
    x87!(x, exp2_tail!())
}

pub fn log_f(x: f64) -> f64 {
    if x < 0.0 || x.is_nan() {
        set_errno(EDOM);
        return f64::NAN;
    }
    if x == 0.0 {
        set_errno(ERANGE);
        return f64::NEG_INFINITY;
    }
    x87!(x, "fldln2", "fxch st(1)", "fyl2x")
}

#[unsafe(no_mangle)]
pub extern "C" fn log(x: f64) -> f64 {
    log_f(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn log10(x: f64) -> f64 {
    if x < 0.0 || x.is_nan() {
        set_errno(EDOM);
        return f64::NAN;
    }
    if x == 0.0 {
        set_errno(ERANGE);
        return f64::NEG_INFINITY;
    }
    x87!(x, "fldlg2", "fxch st(1)", "fyl2x")
}

#[unsafe(no_mangle)]
pub extern "C" fn log2(x: f64) -> f64 {
    if x < 0.0 || x.is_nan() {
        set_errno(EDOM);
        return f64::NAN;
    }
    if x == 0.0 {
        set_errno(ERANGE);
        return f64::NEG_INFINITY;
    }
    x87!(x, "fld1", "fxch st(1)", "fyl2x")
}

#[unsafe(no_mangle)]
pub extern "C" fn log1p(x: f64) -> f64 {
    if x.abs() < 1e-5 {
        return x - x * x / 2.0 + x * x * x / 3.0;
    }
    log_f(1.0 + x)
}

#[unsafe(no_mangle)]
pub extern "C" fn expm1(x: f64) -> f64 {
    if x.abs() < 1e-5 {
        return x + x * x / 2.0 + x * x * x / 6.0;
    }
    exp_f(x) - 1.0
}

pub fn trunc_f(x: f64) -> f64 {
    if !x.is_finite() || x.abs() >= 4503599627370496.0 {
        return x;
    }
    let t = x as i64 as f64;
    if t == 0.0 && x.is_sign_negative() { -0.0 } else { t }
}

pub fn floor_f(x: f64) -> f64 {
    let t = trunc_f(x);
    if t > x { t - 1.0 } else { t }
}

pub fn ceil_f(x: f64) -> f64 {
    let t = trunc_f(x);
    if t < x { t + 1.0 } else { t }
}

#[unsafe(no_mangle)]
pub extern "C" fn floor(x: f64) -> f64 {
    floor_f(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn ceil(x: f64) -> f64 {
    ceil_f(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn trunc(x: f64) -> f64 {
    trunc_f(x)
}

/// round: halves away from zero.
#[unsafe(no_mangle)]
pub extern "C" fn round(x: f64) -> f64 {
    let t = trunc_f(x);
    if (x - t).abs() >= 0.5 { t + x.signum() } else { t }
}

#[unsafe(no_mangle)]
pub extern "C" fn lround(x: f64) -> i32 {
    round(x) as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn llround(x: f64) -> i64 {
    round(x) as i64
}

/// rint: to nearest even (the default rounding mode).
#[unsafe(no_mangle)]
pub extern "C" fn rint(x: f64) -> f64 {
    x87!(x, "frndint")
}

#[unsafe(no_mangle)]
pub extern "C" fn nearbyint(x: f64) -> f64 {
    rint(x)
}

#[unsafe(no_mangle)]
pub extern "C" fn lrint(x: f64) -> i32 {
    rint(x) as i32
}

pub fn fmod_f(x: f64, y: f64) -> f64 {
    if y == 0.0 || x.is_infinite() || x.is_nan() || y.is_nan() {
        set_errno(EDOM);
        return f64::NAN;
    }
    let mut v = x;
    let yy = y;
    unsafe {
        asm!(
            "fld qword ptr [{q}]",
            "fld qword ptr [{p}]",
            "2:",
            "fprem",
            "fnstsw ax",
            "test ah, 4",
            "jnz 2b",
            "fstp qword ptr [{p}]",
            "fstp st(0)",
            p = in(reg) &mut v as *mut f64,
            q = in(reg) &yy as *const f64,
            out("eax") _,
            out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
            out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            options(nostack)
        );
    }
    v
}

#[unsafe(no_mangle)]
pub extern "C" fn fmod(x: f64, y: f64) -> f64 {
    fmod_f(x, y)
}

#[unsafe(no_mangle)]
pub extern "C" fn remainder(x: f64, y: f64) -> f64 {
    let r = fmod_f(x, y);
    if r.abs() > y.abs() / 2.0 { r - y.abs() * r.signum() } else { r }
}

fn is_int(y: f64) -> bool {
    y.is_finite() && trunc_f(y) == y
}

fn is_odd(y: f64) -> bool {
    is_int(y) && y.abs() < 9007199254740992.0 && (y as i64) & 1 == 1
}

pub fn pow_f(x: f64, y: f64) -> f64 {
    if y == 0.0 || x == 1.0 {
        return 1.0;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        if y < 0.0 {
            set_errno(ERANGE);
            return if is_odd(y) && x.is_sign_negative() { f64::NEG_INFINITY } else { f64::INFINITY };
        }
        return if is_odd(y) { x } else { 0.0 };
    }
    if x < 0.0 && !is_int(y) {
        set_errno(EDOM);
        return f64::NAN;
    }
    // Small integer powers exactly.
    if is_int(y) && y.abs() <= 64.0 {
        let mut n = y.abs() as u32;
        let mut b = x;
        let mut r = 1.0;
        while n > 0 {
            if n & 1 == 1 {
                r *= b;
            }
            b *= b;
            n >>= 1;
        }
        return if y < 0.0 { 1.0 / r } else { r };
    }
    let mut v = x.abs();
    let yy = y;
    unsafe {
        asm!(
            "fld qword ptr [{q}]",
            "fld qword ptr [{p}]",
            "fyl2x",
            exp2_tail!(),
            "fstp qword ptr [{p}]",
            p = in(reg) &mut v as *mut f64,
            q = in(reg) &yy as *const f64,
            out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
            out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            options(nostack)
        );
    }
    if v.is_infinite() {
        set_errno(ERANGE);
    }
    if x < 0.0 && is_odd(y) { -v } else { v }
}

#[unsafe(no_mangle)]
pub extern "C" fn pow(x: f64, y: f64) -> f64 {
    pow_f(x, y)
}

#[unsafe(no_mangle)]
pub extern "C" fn cbrt(x: f64) -> f64 {
    let r = pow_f(x.abs(), 1.0 / 3.0);
    if x < 0.0 { -r } else { r }
}

#[unsafe(no_mangle)]
pub extern "C" fn sinh(x: f64) -> f64 {
    let e = exp_f(x);
    (e - 1.0 / e) / 2.0
}

#[unsafe(no_mangle)]
pub extern "C" fn cosh(x: f64) -> f64 {
    let e = exp_f(x);
    (e + 1.0 / e) / 2.0
}

#[unsafe(no_mangle)]
pub extern "C" fn tanh(x: f64) -> f64 {
    if x > 20.0 {
        return 1.0;
    }
    if x < -20.0 {
        return -1.0;
    }
    let e = exp_f(2.0 * x);
    (e - 1.0) / (e + 1.0)
}

#[unsafe(no_mangle)]
pub extern "C" fn asinh(x: f64) -> f64 {
    let r = log_f(x.abs() + sqrt_f(x * x + 1.0));
    if x < 0.0 { -r } else { r }
}

#[unsafe(no_mangle)]
pub extern "C" fn acosh(x: f64) -> f64 {
    log_f(x + sqrt_f(x * x - 1.0))
}

#[unsafe(no_mangle)]
pub extern "C" fn atanh(x: f64) -> f64 {
    0.5 * log_f((1.0 + x) / (1.0 - x))
}

#[unsafe(no_mangle)]
pub extern "C" fn fabs(x: f64) -> f64 {
    x.abs()
}

#[unsafe(no_mangle)]
pub extern "C" fn hypot(x: f64, y: f64) -> f64 {
    let (a, b) = (x.abs(), y.abs());
    let (m, n) = if a > b { (a, b) } else { (b, a) };
    if m == 0.0 || m.is_infinite() {
        return m;
    }
    let r = n / m;
    m * sqrt_f(1.0 + r * r)
}

#[unsafe(no_mangle)]
pub extern "C" fn _hypot(x: f64, y: f64) -> f64 {
    hypot(x, y)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn modf(x: f64, ip: *mut f64) -> f64 {
    let t = trunc_f(x);
    unsafe { ip.write_unaligned(t) };
    if x.is_infinite() { 0.0f64.copysign(x) } else { x - t }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn frexp(x: f64, e: *mut i32) -> f64 {
    if x == 0.0 || !x.is_finite() {
        unsafe { *e = 0 };
        return x;
    }
    let bits = x.to_bits();
    let mut exp = ((bits >> 52) & 0x7ff) as i32;
    let mut b = bits;
    if exp == 0 {
        // Subnormal: normalise first.
        let y = x * 18014398509481984.0; // 2^54
        b = y.to_bits();
        exp = ((b >> 52) & 0x7ff) as i32 - 54;
    }
    unsafe { *e = exp - 1022 };
    f64::from_bits((b & !(0x7ffu64 << 52)) | (1022u64 << 52))
}

#[unsafe(no_mangle)]
pub extern "C" fn ldexp(x: f64, n: i32) -> f64 {
    let mut v = x;
    let nn = n;
    unsafe {
        asm!(
            "fild dword ptr [{q}]",
            "fld qword ptr [{p}]",
            "fscale",
            "fstp qword ptr [{p}]",
            "fstp st(0)",
            p = in(reg) &mut v as *mut f64,
            q = in(reg) &nn as *const i32,
            out("st(0)") _, out("st(1)") _, out("st(2)") _, out("st(3)") _,
            out("st(4)") _, out("st(5)") _, out("st(6)") _, out("st(7)") _,
            options(nostack)
        );
    }
    if v.is_infinite() && x.is_finite() {
        set_errno(ERANGE);
    }
    v
}

#[unsafe(no_mangle)]
pub extern "C" fn scalbn(x: f64, n: i32) -> f64 {
    ldexp(x, n)
}

#[unsafe(no_mangle)]
pub extern "C" fn copysign(x: f64, y: f64) -> f64 {
    x.copysign(y)
}

#[unsafe(no_mangle)]
pub extern "C" fn _copysign(x: f64, y: f64) -> f64 {
    x.copysign(y)
}

#[unsafe(no_mangle)]
pub extern "C" fn _chgsign(x: f64) -> f64 {
    -x
}

#[unsafe(no_mangle)]
pub extern "C" fn fmax(x: f64, y: f64) -> f64 {
    if x.is_nan() || y > x { y } else { x }
}

#[unsafe(no_mangle)]
pub extern "C" fn fmin(x: f64, y: f64) -> f64 {
    if x.is_nan() || y < x { y } else { x }
}

#[unsafe(no_mangle)]
pub extern "C" fn _isnan(x: f64) -> i32 {
    x.is_nan() as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn _finite(x: f64) -> i32 {
    x.is_finite() as i32
}

/// _fpclass: _FPCLASS_* bits.
#[unsafe(no_mangle)]
pub extern "C" fn _fpclass(x: f64) -> i32 {
    let neg = x.is_sign_negative();
    match (x.classify(), neg) {
        (core::num::FpCategory::Nan, _) => 0x0002,
        (core::num::FpCategory::Infinite, true) => 0x0004,
        (core::num::FpCategory::Infinite, false) => 0x0200,
        (core::num::FpCategory::Normal, true) => 0x0008,
        (core::num::FpCategory::Normal, false) => 0x0100,
        (core::num::FpCategory::Subnormal, true) => 0x0010,
        (core::num::FpCategory::Subnormal, false) => 0x0080,
        (core::num::FpCategory::Zero, true) => 0x0020,
        (core::num::FpCategory::Zero, false) => 0x0040,
    }
}

/// _dclass / fpclassify for the UCRT: FP_NAN 2, FP_INFINITE 1, FP_ZERO 0,
/// FP_SUBNORMAL -2, FP_NORMAL -1.
#[unsafe(no_mangle)]
pub extern "C" fn _dclass(x: f64) -> i16 {
    match x.classify() {
        core::num::FpCategory::Nan => 2,
        core::num::FpCategory::Infinite => 1,
        core::num::FpCategory::Zero => 0,
        core::num::FpCategory::Subnormal => -2,
        core::num::FpCategory::Normal => -1,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _dsign(x: f64) -> i32 {
    x.is_sign_negative() as i32
}

// float forms (the UCRT exports them).

#[unsafe(no_mangle)]
pub extern "C" fn sqrtf(x: f32) -> f32 {
    sqrt_f(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn sinf(x: f32) -> f32 {
    sin(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn cosf(x: f32) -> f32 {
    cos(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn tanf(x: f32) -> f32 {
    tan(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn atan2f(y: f32, x: f32) -> f32 {
    atan2_f(y as f64, x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn expf(x: f32) -> f32 {
    exp_f(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn logf(x: f32) -> f32 {
    log_f(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn powf(x: f32, y: f32) -> f32 {
    pow_f(x as f64, y as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn floorf(x: f32) -> f32 {
    floor_f(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn ceilf(x: f32) -> f32 {
    ceil_f(x as f64) as f32
}

#[unsafe(no_mangle)]
pub extern "C" fn fmodf(x: f32, y: f32) -> f32 {
    fmod_f(x as f64, y as f64) as f32
}

// ---- The floating point unit's control ----

#[unsafe(no_mangle)]
pub extern "C" fn _fpreset() {
    unsafe { asm!("fninit", options(nostack, nomem)) };
}

#[unsafe(no_mangle)]
pub extern "C" fn _clearfp() -> u32 {
    let mut sw: u16 = 0;
    unsafe { asm!("fnstsw word ptr [{p}]", "fnclex", p = in(reg) &mut sw as *mut u16, options(nostack)) };
    sw as u32 & 0x3f
}

#[unsafe(no_mangle)]
pub extern "C" fn _statusfp() -> u32 {
    let mut sw: u16 = 0;
    unsafe { asm!("fnstsw word ptr [{p}]", p = in(reg) &mut sw as *mut u16, options(nostack)) };
    sw as u32 & 0x3f
}

/// _control87(new, mask): the x87 control word (Microsoft's bit layout
/// differs from the hardware's; programs mostly ask for the rounding and
/// precision, which are only passed through as they are).
#[unsafe(no_mangle)]
pub extern "C" fn _control87(new: u32, mask: u32) -> u32 {
    let mut cw: u16 = 0;
    unsafe { asm!("fnstcw word ptr [{p}]", p = in(reg) &mut cw as *mut u16, options(nostack)) };
    // Hardware exception masks (bits 0-5) are Microsoft's _EM_* bits
    // reversed in order; report them as Microsoft's.
    let hw = cw as u32;
    let ms_old = ((hw & 1) << 4) | ((hw & 2) << 18) | ((hw & 4) << 1) | ((hw & 8) >> 1) | ((hw & 16) >> 3) | ((hw & 32) >> 5) | ((hw >> 10) & 3) << 8 | ((hw >> 8) & 3) << 16;
    if mask != 0 {
        let ms = (ms_old & !mask) | (new & mask);
        let mut n = cw as u32 & !0x0f3f;
        n |= ((ms >> 4) & 1) | ((ms >> 18) & 2) | ((ms >> 1) & 4) | ((ms << 1) & 8) | ((ms << 3) & 16) | ((ms << 5) & 32);
        n |= ((ms >> 8) & 3) << 10 | ((ms >> 16) & 3) << 8;
        let ncw = n as u16;
        unsafe { asm!("fldcw word ptr [{p}]", p = in(reg) &ncw as *const u16, options(nostack)) };
        return ms;
    }
    ms_old
}

#[unsafe(no_mangle)]
pub extern "C" fn _controlfp(new: u32, mask: u32) -> u32 {
    _control87(new, mask & !0x0008_0000)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn _controlfp_s(cur: *mut u32, new: u32, mask: u32) -> i32 {
    let r = _controlfp(new, mask);
    if !cur.is_null() {
        unsafe { *cur = r };
    }
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn __fpecode() -> *mut i32 {
    static mut C: i32 = 0;
    &raw mut C
}

/// _matherr: the default handler (no change).
#[unsafe(no_mangle)]
pub extern "C" fn _matherr(_e: u32) -> i32 {
    0
}
