//! The C math library (std's f64/f32 methods call it): the `libm` crate's
//! functions under their C names.

macro_rules! export {
    ($($name:ident($($a:ident: $t:ty),*) -> $r:ty;)*) => {
        $(
            #[unsafe(no_mangle)]
            pub extern "C" fn $name($($a: $t),*) -> $r {
                libm::$name($($a),*)
            }
        )*
    };
}

export! {
    acos(x: f64) -> f64;
    acosh(x: f64) -> f64;
    acoshf(x: f32) -> f32;
    asinh(x: f64) -> f64;
    asinhf(x: f32) -> f32;
    atanh(x: f64) -> f64;
    atanhf(x: f32) -> f32;
    acosf(x: f32) -> f32;
    asin(x: f64) -> f64;
    asinf(x: f32) -> f32;
    atan(x: f64) -> f64;
    atanf(x: f32) -> f32;
    atan2(y: f64, x: f64) -> f64;
    atan2f(y: f32, x: f32) -> f32;
    cbrt(x: f64) -> f64;
    cbrtf(x: f32) -> f32;
    ceil(x: f64) -> f64;
    ceilf(x: f32) -> f32;
    cos(x: f64) -> f64;
    cosf(x: f32) -> f32;
    cosh(x: f64) -> f64;
    coshf(x: f32) -> f32;
    exp(x: f64) -> f64;
    expf(x: f32) -> f32;
    exp2(x: f64) -> f64;
    exp2f(x: f32) -> f32;
    expm1(x: f64) -> f64;
    expm1f(x: f32) -> f32;
    fabs(x: f64) -> f64;
    fabsf(x: f32) -> f32;
    fdim(x: f64, y: f64) -> f64;
    fdimf(x: f32, y: f32) -> f32;
    floor(x: f64) -> f64;
    floorf(x: f32) -> f32;
    fma(x: f64, y: f64, z: f64) -> f64;
    fmaf(x: f32, y: f32, z: f32) -> f32;
    fmax(x: f64, y: f64) -> f64;
    fmaxf(x: f32, y: f32) -> f32;
    fmin(x: f64, y: f64) -> f64;
    fminf(x: f32, y: f32) -> f32;
    fmod(x: f64, y: f64) -> f64;
    fmodf(x: f32, y: f32) -> f32;
    hypot(x: f64, y: f64) -> f64;
    hypotf(x: f32, y: f32) -> f32;
    log(x: f64) -> f64;
    logf(x: f32) -> f32;
    log10(x: f64) -> f64;
    log10f(x: f32) -> f32;
    log1p(x: f64) -> f64;
    log1pf(x: f32) -> f32;
    log2(x: f64) -> f64;
    log2f(x: f32) -> f32;
    pow(x: f64, y: f64) -> f64;
    powf(x: f32, y: f32) -> f32;
    round(x: f64) -> f64;
    roundf(x: f32) -> f32;
    sin(x: f64) -> f64;
    sinf(x: f32) -> f32;
    sinh(x: f64) -> f64;
    sinhf(x: f32) -> f32;
    sqrt(x: f64) -> f64;
    sqrtf(x: f32) -> f32;
    tan(x: f64) -> f64;
    tanf(x: f32) -> f32;
    tanh(x: f64) -> f64;
    tanhf(x: f32) -> f32;
    trunc(x: f64) -> f64;
    truncf(x: f32) -> f32;
}
