/* <math.h>. The transcendental functions use the x87 unit, computing in
 * 80-bit precision; the float and long double forms use the double ones
 * (long double results are double precision). */
#include "libc.h"
#include <math.h>
#include <stdint.h>

typedef long double ld;

static int domain(void) {
    errno = EDOM;
    return 0;
}

static double range(double r) {
    errno = ERANGE;
    return r;
}

/* ---- x87 primitives ---- */

static ld x87_log2(ld x) { ld r; __asm__("fld1; fxch; fyl2x" : "=t"(r) : "0"(x)); return r; }
static ld x87_ln(ld x) { ld r; __asm__("fldln2; fxch; fyl2x" : "=t"(r) : "0"(x)); return r; }
static ld x87_lg(ld x) { ld r; __asm__("fldlg2; fxch; fyl2x" : "=t"(r) : "0"(x)); return r; }
static ld x87_lnp1(ld x) { ld r; __asm__("fldln2; fxch; fyl2xp1" : "=t"(r) : "0"(x)); return r; }
static ld x87_ylog2(ld y, ld x) { ld r; __asm__("fyl2x" : "=t"(r) : "0"(x), "u"(y) : "st(1)"); return r; }
static ld x87_rnd(ld x) { ld r; __asm__("frndint" : "=t"(r) : "0"(x)); return r; }
static ld x87_scale(ld x, ld e) { ld r; __asm__("fscale" : "=t"(r) : "0"(x), "u"(e)); return r; }
static ld x87_2xm1(ld x) { ld r; __asm__("f2xm1" : "=t"(r) : "0"(x)); return r; }
static ld x87_atan2(ld y, ld x) { ld r; __asm__("fpatan" : "=t"(r) : "0"(x), "u"(y) : "st(1)"); return r; }

/* 2^t for any t. */
static ld exp2_ld(ld t) {
    if (t != t)
        return t;
    if (t > 20000)
        return __builtin_infl();
    if (t < -20000)
        return 0;
    ld i = x87_rnd(t);
    return x87_scale(x87_2xm1(t - i) + 1, i);
}

/* fprem1 until done: x mod 2pi, near zero. */
static ld reduce(ld x) {
    static const ld twopi = 6.283185307179586476925286766559L;
    ld r;
    unsigned short sw;
    do {
        __asm__("fprem1; fnstsw %%ax" : "=t"(r), "=a"(sw) : "0"(x), "u"(twopi));
        x = r;
    } while (sw & 0x400);
    return x;
}

static ld sin_ld(ld x) {
    ld r;
    unsigned short sw;
    __asm__("fsin; fnstsw %%ax" : "=t"(r), "=a"(sw) : "0"(x));
    if (sw & 0x400)
        __asm__("fsin" : "=t"(r) : "0"(reduce(x)));
    return r;
}

static ld cos_ld(ld x) {
    ld r;
    unsigned short sw;
    __asm__("fcos; fnstsw %%ax" : "=t"(r), "=a"(sw) : "0"(x));
    if (sw & 0x400)
        __asm__("fcos" : "=t"(r) : "0"(reduce(x)));
    return r;
}

/* ---- exponentials and logarithms ---- */

double exp(double x) {
    ld r = exp2_ld((ld)x * 1.4426950408889634073599246810019L);
    if (r > 1.7976931348623157e308L || (r == 0 && x == x && x != -__builtin_inf()))
        return range((double)r);
    return (double)r;
}

double exp2(double x) {
    ld r = exp2_ld(x);
    if (r > 1.7976931348623157e308L)
        return range((double)r);
    return (double)r;
}

double expm1(double x) {
    if (x != x)
        return x;
    ld t = (ld)x * 1.4426950408889634073599246810019L;
    if (t > -1 && t < 1)
        return (double)x87_2xm1(t);
    return (double)(exp2_ld(t) - 1);
}

double log(double x) {
    if (x < 0)
        return domain(), __builtin_nan("");
    if (x == 0)
        return range(-__builtin_inf());
    return (double)x87_ln(x);
}

double log2(double x) {
    if (x < 0)
        return domain(), __builtin_nan("");
    if (x == 0)
        return range(-__builtin_inf());
    return (double)x87_log2(x);
}

double log10(double x) {
    if (x < 0)
        return domain(), __builtin_nan("");
    if (x == 0)
        return range(-__builtin_inf());
    return (double)x87_lg(x);
}

double log1p(double x) {
    if (x < -1)
        return domain(), __builtin_nan("");
    if (x == -1)
        return range(-__builtin_inf());
    if (x > -0.29 && x < 0.29)
        return (double)x87_lnp1(x);
    return (double)x87_ln((ld)x + 1);
}

double logb(double x) {
    if (x == 0)
        return range(-__builtin_inf());
    if (!__builtin_isfinite(x))
        return x * x;
    return (double)ilogb(x);
}

int ilogb(double x) {
    if (x == 0 || x != x)
        return FP_ILOGB0;
    if (!__builtin_isfinite(x))
        return 0x7fffffff;
    int e;
    frexp(x, &e);
    return e - 1;
}

static int is_int(double y) {
    return __builtin_isfinite(y) && y == (double)(ld)trunc(y);
}

static int is_odd_int(double y) {
    if (!is_int(y) || fabs(y) >= 9007199254740992.0)
        return 0;
    return ((long long)y) & 1;
}

double pow(double x, double y) {
    if (y == 0 || x == 1)
        return 1;
    if (x != x || y != y)
        return x + y;
    if (x == 0) {
        if (y < 0) {
            errno = ERANGE;
            return is_odd_int(y) ? copysign(__builtin_inf(), x) : __builtin_inf();
        }
        return is_odd_int(y) ? x : 0;
    }
    if (x == -1 && !__builtin_isfinite(y))
        return 1;
    if (!__builtin_isfinite(y)) {
        int big = fabs(x) > 1;
        return (y > 0) == big ? __builtin_inf() : 0;
    }
    if (!__builtin_isfinite(x)) {
        if (x > 0)
            return y > 0 ? x : 0;
        if (y > 0)
            return is_odd_int(y) ? x : __builtin_inf();
        return is_odd_int(y) ? -0.0 : 0;
    }
    int neg = 0;
    if (x < 0) {
        if (!is_int(y))
            return domain(), __builtin_nan("");
        neg = is_odd_int(y);
        x = -x;
    }
    ld r = exp2_ld(x87_ylog2(y, x));
    double d = (double)r;
    if (__builtin_isinf(d) || (d == 0 && r != 0) || r == 0)
        errno = ERANGE;
    return neg ? -d : d;
}

double sqrt(double x) {
    if (x < 0)
        return domain(), __builtin_nan("");
    double r;
    __asm__("sqrtsd %1, %0" : "=x"(r) : "x"(x));
    return r;
}

double cbrt(double x) {
    if (x == 0 || !__builtin_isfinite(x))
        return x;
    ld a = fabs(x);
    ld r = exp2_ld(x87_log2(a) / 3);
    r = r - (r * r * r - a) / (3 * r * r);   /* one Newton step */
    return x < 0 ? -(double)r : (double)r;
}

double hypot(double x, double y) {
    if (__builtin_isinf(x) || __builtin_isinf(y))
        return __builtin_inf();
    if (x != x || y != y)
        return x + y;
    ld r = (ld)x * x + (ld)y * y;
    ld s;
    __asm__("fsqrt" : "=t"(s) : "0"(r));
    double d = (double)s;
    if (__builtin_isinf(d))
        errno = ERANGE;
    return d;
}

/* ---- trigonometry ---- */

double sin(double x) {
    if (__builtin_isinf(x))
        return domain(), __builtin_nan("");
    return (double)sin_ld(x);
}

double cos(double x) {
    if (__builtin_isinf(x))
        return domain(), __builtin_nan("");
    return (double)cos_ld(x);
}

double tan(double x) {
    if (__builtin_isinf(x))
        return domain(), __builtin_nan("");
    return (double)(sin_ld(x) / cos_ld(x));
}

double atan(double x) {
    return (double)x87_atan2(x, 1.0L);
}

double atan2(double y, double x) {
    if (x != x || y != y)
        return x + y;
    return (double)x87_atan2(y, x);
}

double asin(double x) {
    if (x < -1 || x > 1)
        return domain(), __builtin_nan("");
    ld s, t = (1 - (ld)x) * (1 + (ld)x);
    __asm__("fsqrt" : "=t"(s) : "0"(t));
    return (double)x87_atan2(x, s);
}

double acos(double x) {
    if (x < -1 || x > 1)
        return domain(), __builtin_nan("");
    ld s, t = (1 - (ld)x) * (1 + (ld)x);
    __asm__("fsqrt" : "=t"(s) : "0"(t));
    return (double)x87_atan2(s, x);
}

double sinh(double x) {
    if (fabs(x) < 0.5) {
        ld e = expm1(x);
        return (double)((e + e / (e + 1)) / 2);
    }
    ld e = exp2_ld((ld)x * 1.4426950408889634073599246810019L);
    double r = (double)((e - 1 / e) / 2);
    if (__builtin_isinf(r) && __builtin_isfinite(x))
        errno = ERANGE;
    return r;
}

double cosh(double x) {
    ld e = exp2_ld(fabs(x) * 1.4426950408889634073599246810019L);
    double r = (double)((e + 1 / e) / 2);
    if (__builtin_isinf(r) && __builtin_isfinite(x))
        errno = ERANGE;
    return r;
}

double tanh(double x) {
    if (x != x)
        return x;
    if (fabs(x) > 23)
        return x > 0 ? 1 : -1;
    ld e = expm1(2 * fabs(x));
    double r = (double)(e / (e + 2));
    return x < 0 ? -r : r;
}

double asinh(double x) {
    double a = fabs(x);
    ld r;
    if (a > 1e10)
        r = x87_ln(a) + 0.69314718055994530941723212145818L;
    else
        r = log1p(a + (ld)a * a / (1 + sqrt(1 + (double)((ld)a * a))));
    return x < 0 ? -(double)r : (double)r;
}

double acosh(double x) {
    if (x < 1)
        return domain(), __builtin_nan("");
    if (x > 1e10)
        return (double)(x87_ln(x) + 0.69314718055994530941723212145818L);
    ld t = (ld)x * x - 1, s;
    __asm__("fsqrt" : "=t"(s) : "0"(t));
    return (double)x87_ln(x + s);
}

double atanh(double x) {
    if (x < -1 || x > 1)
        return domain(), __builtin_nan("");
    if (x == 1 || x == -1)
        return range(copysign(__builtin_inf(), x));
    return (double)(0.5L * x87_lnp1(2 * (ld)x / (1 - (ld)x)));
}

/* ---- error and gamma functions ---- */

double erf(double x) {
    if (x != x)
        return x;
    double a = fabs(x);
    if (a > 6)
        return x > 0 ? 1 : -1;
    if (a < 2.5) {
        /* The Taylor series, in long double. */
        ld sum = 0, term = a, x2 = (ld)a * a;
        for (int n = 0; n < 200; n++) {
            ld t = term / (2 * n + 1);
            sum += t;
            if (t < 1e-22L * sum)
                break;
            term *= -x2 / (n + 1);
        }
        double r = (double)(sum * 1.1283791670955125738961589031215L);
        return x < 0 ? -r : r;
    }
    double r = 1 - erfc(a);
    return x < 0 ? -r : r;
}

double erfc(double x) {
    if (x != x)
        return x;
    if (x < 2.5)
        return 1 - erf(x);
    if (x > 27)
        return range(0);
    /* The continued fraction (modified Lentz). */
    ld z = x, f, c, d, tiny = 1e-300L;
    f = z;
    c = z;
    d = 0;
    for (int n = 1; n < 500; n++) {
        ld an = n / 2.0L;
        d = z + an * d;
        d = d == 0 ? tiny : d;
        c = z + an / c;
        c = c == 0 ? tiny : c;
        d = 1 / d;
        ld delta = c * d;
        f *= delta;
        if (fabsl(delta - 1) < 1e-20L)
            break;
    }
    ld e = exp2_ld(-z * z * 1.4426950408889634073599246810019L);
    return (double)(e / f * 0.56418958354775628694807945156077L);
}

/* Lanczos, g = 7, nine terms. */
static const double lanczos[9] = {0.99999999999980993, 676.5203681218851, -1259.1392167224028,
                                  771.32342877765313, -176.61502916214059, 12.507343278686905,
                                  -0.13857109526572012, 9.9843695780195716e-6, 1.5056327351493116e-7};

int signgam;

static ld lgamma_pos(ld x) {   /* x >= 0.5 */
    if (x > 1e17L)
        return x * (x87_ln(x) - 1);
    x -= 1;
    ld a = lanczos[0], t = x + 7.5L;
    for (int i = 1; i < 9; i++)
        a += lanczos[i] / (x + i);
    return 0.91893853320467274178032973640562L + (x + 0.5L) * x87_ln(t) - t + x87_ln(a);
}

double lgamma(double x) {
    signgam = 1;
    if (x != x)
        return x;
    if (__builtin_isinf(x))
        return __builtin_inf();
    if (x <= 0 && x == trunc(x))
        return range(__builtin_inf());
    if (x < 0.5) {
        /* Reflection: Gamma(x) Gamma(1-x) = pi / sin(pi x). */
        ld s = sin_ld(3.1415926535897932384626433832795L * x);
        if (s < 0)
            signgam = -1;
        return (double)(1.1447298858494001741434273513531L - x87_ln(s < 0 ? -s : s) - lgamma_pos(1 - (ld)x));
    }
    return (double)lgamma_pos(x);
}

double tgamma(double x) {
    if (x != x)
        return x;
    if (x == 0)
        return range(copysign(__builtin_inf(), x));
    if (x < 0 && x == trunc(x))
        return domain(), __builtin_nan("");
    if (x == __builtin_inf())
        return x;
    if (x == -__builtin_inf())
        return domain(), __builtin_nan("");
    if (x > 171.7)
        return range(__builtin_inf());
    double l = lgamma(x);
    ld r = exp2_ld((ld)l * 1.4426950408889634073599246810019L);
    return signgam < 0 ? -(double)r : (double)r;
}

/* ---- rounding and pieces ---- */

double fabs(double x) {
    union { double d; uint64_t u; } v = {x};
    v.u &= ~(1ULL << 63);
    return v.d;
}

double copysign(double x, double y) {
    union { double d; uint64_t u; } a = {x}, b = {y};
    a.u = (a.u & ~(1ULL << 63)) | (b.u & (1ULL << 63));
    return a.d;
}

double trunc(double x) {
    if (!(fabs(x) < 4503599627370496.0))
        return x;
    return copysign((double)(long long)x, x);
}

double floor(double x) {
    double t = trunc(x);
    return t > x ? t - 1 : t;
}

double ceil(double x) {
    double t = trunc(x);
    return t < x ? copysign(t + 1, x) : t;
}

double round(double x) {
    double t = trunc(x);
    if (fabs(x - t) >= 0.5)
        t += copysign(1.0, x);
    return t;
}

double rint(double x) {
    if (!(fabs(x) < 4503599627370496.0))
        return x;
    volatile double big = copysign(4503599627370496.0, x);
    double r = (x + big) - big;
    return copysign(r, x);
}

double nearbyint(double x) {
    return rint(x);
}

long lrint(double x) { return (long)rint(x); }
long long llrint(double x) { return (long long)rint(x); }
long lround(double x) { return (long)round(x); }
long long llround(double x) { return (long long)round(x); }

double fmod(double x, double y) {
    if (y == 0 || __builtin_isinf(x))
        return domain(), __builtin_nan("");
    if (x != x || y != y)
        return x + y;
    ld r;
    unsigned short sw;
    ld a = x;
    do {
        __asm__("fprem; fnstsw %%ax" : "=t"(r), "=a"(sw) : "0"(a), "u"((ld)y));
        a = r;
    } while (sw & 0x400);
    return (double)r;
}

double remainder(double x, double y) {
    if (y == 0 || __builtin_isinf(x))
        return domain(), __builtin_nan("");
    if (x != x || y != y)
        return x + y;
    ld r;
    unsigned short sw;
    ld a = x;
    do {
        __asm__("fprem1; fnstsw %%ax" : "=t"(r), "=a"(sw) : "0"(a), "u"((ld)y));
        a = r;
    } while (sw & 0x400);
    return (double)r;
}

double remquo(double x, double y, int *q) {
    double r = remainder(x, y);
    *q = (int)((long long)round((x - r) / y) & 0x7fffffff) * ((x < 0) != (y < 0) ? -1 : 1);
    return r;
}

double frexp(double x, int *e) {
    union { double d; uint64_t u; } v = {x};
    int ex = (int)(v.u >> 52 & 0x7ff);
    if (ex == 0) {
        if (x == 0) {
            *e = 0;
            return x;
        }
        v.d = x * 18014398509481984.0;   /* 2^54 */
        ex = (int)(v.u >> 52 & 0x7ff) - 54;
    } else if (ex == 0x7ff) {
        *e = 0;
        return x;
    }
    *e = ex - 1022;
    v.u = (v.u & ~(0x7ffULL << 52)) | (1022ULL << 52);
    return v.d;
}

double scalbn(double x, int n) {
    double r = (double)x87_scale(x, n < -30000 ? -30000 : n > 30000 ? 30000 : n);
    if (x != 0 && __builtin_isfinite(x) && (__builtin_isinf(r) || r == 0))
        errno = ERANGE;
    return r;
}

double ldexp(double x, int n) { return scalbn(x, n); }
double scalbln(double x, long n) { return scalbn(x, n < -30000 ? -30000 : n > 30000 ? 30000 : (int)n); }

double modf(double x, double *ip) {
    double t = trunc(x);
    *ip = t;
    if (__builtin_isinf(x))
        return copysign(0.0, x);
    return copysign(x - t, x);
}

double nextafter(double x, double y) {
    if (x != x || y != y)
        return x + y;
    if (x == y)
        return y;
    union { double d; uint64_t u; } v = {x};
    if (x == 0)
        v.u = 1 | (y < 0 ? 1ULL << 63 : 0);
    else if ((x < y) == (x > 0))
        v.u++;
    else
        v.u--;
    return v.d;
}

double fdim(double x, double y) { return x > y ? x - y : (x != x || y != y) ? x + y : 0; }
double fmax(double x, double y) { return x != x ? y : y != y ? x : x > y ? x : y; }
double fmin(double x, double y) { return x != x ? y : y != y ? x : x < y ? x : y; }
double fma(double x, double y, double z) { return (double)((ld)x * y + z); }
float fmaf(float x, float y, float z) { return (float)((double)x * y + z); }

double nan(const char *s) { (void)s; return __builtin_nan(""); }
float nanf(const char *s) { (void)s; return __builtin_nanf(""); }
long double nanl(const char *s) { (void)s; return __builtin_nanl(""); }

/* ---- float and long double forms ---- */

#define F1(n) float n##f(float x) { return (float)n(x); } long double n##l(long double x) { return n((double)x); }
#define F2(n) float n##f(float x, float y) { return (float)n(x, y); } \
    long double n##l(long double x, long double y) { return n((double)x, (double)y); }
F1(acos) F1(asin) F1(atan) F2(atan2) F1(cos) F1(sin) F1(tan)
F1(acosh) F1(asinh) F1(atanh) F1(cosh) F1(sinh) F1(tanh)
F1(exp) F1(exp2) F1(expm1) F1(log) F1(log10) F1(log1p) F1(log2) F1(logb)
F1(cbrt) F1(fabs) F2(hypot) F2(pow) F1(sqrt) F1(erf) F1(erfc) F1(lgamma) F1(tgamma)
F1(ceil) F1(floor) F1(nearbyint) F1(rint) F1(round) F1(trunc)
F2(fmod) F2(remainder) F2(copysign) F2(fdim) F2(fmax) F2(fmin)

float nextafterf(float x, float y) {
    if (x != x || y != y)
        return x + y;
    if (x == y)
        return y;
    union { float f; uint32_t u; } v = {x};
    if (x == 0)
        v.u = 1 | (y < 0 ? 1U << 31 : 0);
    else if ((x < y) == (x > 0))
        v.u++;
    else
        v.u--;
    return v.f;
}
/* In double precision, as the other long double functions. */
long double nextafterl(long double x, long double y) { return nextafter((double)x, (double)y); }
long double scalbnl(long double x, int n) { return x87_scale(x, n); }
float scalblnf(float x, long n) { return (float)scalbln(x, n); }
long double scalblnl(long double x, long n) { return x87_scale(x, n < -40000 ? -40000 : n > 40000 ? 40000 : n); }
int ilogbf(float x) { return ilogb(x); }
int ilogbl(long double x) { return ilogb((double)x); }
long double fmal(long double x, long double y, long double z) { return x * y + z; }
double nexttoward(double x, long double y) { return x == y ? (double)y : nextafter(x, y > x ? __builtin_inf() : -__builtin_inf()); }
float nexttowardf(float x, long double y) { return x == y ? (float)y : nextafterf(x, y > x ? __builtin_inff() : -__builtin_inff()); }
long double nexttowardl(long double x, long double y) { return nextafterl(x, y); }
long lrintf(float x) { return lrint(x); }
long lrintl(long double x) { return (long)x87_rnd(x); }
long long llrintf(float x) { return llrint(x); }
long long llrintl(long double x) { return (long long)x87_rnd(x); }
long lroundf(float x) { return lround(x); }
long lroundl(long double x) { return lround((double)x); }
long long llroundf(float x) { return llround(x); }
long long llroundl(long double x) { return llround((double)x); }
float remquof(float x, float y, int *q) { return (float)remquo(x, y, q); }
long double remquol(long double x, long double y, int *q) { return remquo((double)x, (double)y, q); }
float frexpf(float x, int *e) { return (float)frexp(x, e); }
long double frexpl(long double x, int *e) { return frexp((double)x, e); }
float ldexpf(float x, int n) { return (float)ldexp(x, n); }
long double ldexpl(long double x, int n) { return x87_scale(x, n); }
float scalbnf(float x, int n) { return (float)scalbn(x, n); }
float modff(float x, float *ip) { double i; float r = (float)modf(x, &i); *ip = (float)i; return r; }
long double modfl(long double x, long double *ip) { double i; long double r = modf((double)x, &i); *ip = i; return r; }
