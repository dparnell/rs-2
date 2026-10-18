//! Floating-point helpers missing from `core` (no libm in no_std).

pub const PI: f64 = core::f64::consts::PI;

pub fn floor(x: f64) -> f64 {
    let t = x as i64 as f64;
    if t > x { t - 1.0 } else { t }
}

pub fn ceil(x: f64) -> f64 {
    let t = x as i64 as f64;
    if t < x { t + 1.0 } else { t }
}

/// Round half away from zero, saturating to i32.
pub fn round(x: f64) -> i32 {
    let r = if x >= 0.0 { floor(x + 0.5) } else { -floor(-x + 0.5) };
    r.clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

pub fn abs(x: f64) -> f64 {
    if x < 0.0 { -x } else { x }
}

pub fn sqrt(x: f64) -> f64 {
    if x <= 0.0 || x != x {
        return 0.0;
    }
    // Initial guess from the exponent, then Newton steps.
    let mut g = f64::from_bits((x.to_bits() >> 1) + (1023u64 << 51));
    for _ in 0..6 {
        g = 0.5 * (g + x / g);
    }
    g
}

/// sin and cos of an angle in radians.
pub fn sin_cos(a: f64) -> (f64, f64) {
    // Reduce to [-pi, pi].
    let k = floor(a / (2.0 * PI) + 0.5);
    let mut x = a - k * 2.0 * PI;
    // Reduce to [-pi/2, pi/2] using sin(pi - x) = sin x, cos(pi - x) = -cos x.
    let mut cs = 1.0;
    if x > PI / 2.0 {
        x = PI - x;
        cs = -1.0;
    } else if x < -PI / 2.0 {
        x = -PI - x;
        cs = -1.0;
    }
    let x2 = x * x;
    // Taylor series, accurate to ~1e-12 on [-pi/2, pi/2].
    let s = x * (1.0 - x2 / 6.0 * (1.0 - x2 / 20.0 * (1.0 - x2 / 42.0 * (1.0 - x2 / 72.0 * (1.0 - x2 / 110.0 * (1.0 - x2 / 156.0))))));
    let c = 1.0 - x2 / 2.0 * (1.0 - x2 / 12.0 * (1.0 - x2 / 30.0 * (1.0 - x2 / 56.0 * (1.0 - x2 / 90.0 * (1.0 - x2 / 132.0 * (1.0 - x2 / 182.0))))));
    (s, cs * c)
}

fn atan_small(x: f64) -> f64 {
    // Halve the argument twice: atan x = 2 atan(x / (1 + sqrt(1 + x^2))).
    let mut x = x;
    let mut m = 1.0;
    for _ in 0..3 {
        x /= 1.0 + sqrt(1.0 + x * x);
        m *= 2.0;
    }
    let x2 = x * x;
    let mut term = x;
    let mut sum = 0.0;
    let mut n = 1.0;
    for _ in 0..12 {
        sum += term / n;
        term *= -x2;
        n += 2.0;
    }
    m * sum
}

pub fn atan2(y: f64, x: f64) -> f64 {
    if x == 0.0 && y == 0.0 {
        return 0.0;
    }
    if abs(x) >= abs(y) {
        let a = atan_small(y / x);
        if x > 0.0 {
            a
        } else if y >= 0.0 {
            a + PI
        } else {
            a - PI
        }
    } else {
        let a = atan_small(x / y);
        if y > 0.0 { PI / 2.0 - a } else { -PI / 2.0 - a }
    }
}

/// FIXED (16.16) to f64.
pub fn fx(v: i32) -> f64 {
    v as f64 / 65536.0
}

pub fn to_fx(v: f64) -> i32 {
    round(v * 65536.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f64, b: f64) -> bool {
        abs(a - b) < 1e-9
    }

    #[test]
    fn trig() {
        for i in -40..40 {
            let a = i as f64 * 0.37;
            let (s, c) = sin_cos(a);
            assert!(near(s * s + c * c, 1.0));
            let back = atan2(s, c);
            let d = a - back;
            let k = round(d / (2.0 * PI));
            assert!(near(d - k as f64 * 2.0 * PI, 0.0), "{a} {back}");
        }
        assert!(near(sin_cos(PI / 6.0).0, 0.5));
        assert!(near(sqrt(2.0) * sqrt(2.0), 2.0));
        assert_eq!(round(-2.5), -3);
        assert_eq!(floor(-0.5), -1.0);
        assert_eq!(ceil(1.2), 2.0);
    }
}
