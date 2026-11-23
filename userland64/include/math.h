#ifndef _MATH_H
#define _MATH_H
#include "_rs2libc.h"
_RS2_BEGIN
typedef double double_t;
typedef float float_t;
#define HUGE_VAL __builtin_huge_val()
#define HUGE_VALF __builtin_huge_valf()
#define HUGE_VALL __builtin_huge_vall()
#define INFINITY __builtin_inff()
#define NAN __builtin_nanf("")
#define M_E 2.7182818284590452354
#define M_LOG2E 1.4426950408889634074
#define M_LOG10E 0.43429448190325182765
#define M_LN2 0.69314718055994530942
#define M_LN10 2.30258509299404568402
#define M_PI 3.14159265358979323846
#define M_PI_2 1.57079632679489661923
#define M_PI_4 0.78539816339744830962
#define M_1_PI 0.31830988618379067154
#define M_2_PI 0.63661977236758134308
#define M_2_SQRTPI 1.12837916709551257390
#define M_SQRT2 1.41421356237309504880
#define M_SQRT1_2 0.70710678118654752440
#define FP_NAN 0
#define FP_INFINITE 1
#define FP_ZERO 2
#define FP_SUBNORMAL 3
#define FP_NORMAL 4
#define MATH_ERRNO 1
#define MATH_ERREXCEPT 2
#define math_errhandling MATH_ERRNO
#define FP_ILOGB0 (-2147483647 - 1)
#define FP_ILOGBNAN (-2147483647 - 1)
#define fpclassify(x) __builtin_fpclassify(FP_NAN, FP_INFINITE, FP_NORMAL, FP_SUBNORMAL, FP_ZERO, x)
#define isfinite(x) __builtin_isfinite(x)
#define isinf(x) __builtin_isinf(x)
#define isnan(x) __builtin_isnan(x)
#define isnormal(x) __builtin_isnormal(x)
#define signbit(x) __builtin_signbit(x)
#define isgreater(x, y) __builtin_isgreater(x, y)
#define isgreaterequal(x, y) __builtin_isgreaterequal(x, y)
#define isless(x, y) __builtin_isless(x, y)
#define islessequal(x, y) __builtin_islessequal(x, y)
#define islessgreater(x, y) __builtin_islessgreater(x, y)
#define isunordered(x, y) __builtin_isunordered(x, y)

#define __RS2_MATH1(n) double n(double); float n##f(float); long double n##l(long double);
#define __RS2_MATH2(n) double n(double, double); float n##f(float, float); long double n##l(long double, long double);
__RS2_MATH1(acos) __RS2_MATH1(asin) __RS2_MATH1(atan) __RS2_MATH2(atan2)
__RS2_MATH1(cos) __RS2_MATH1(sin) __RS2_MATH1(tan)
__RS2_MATH1(acosh) __RS2_MATH1(asinh) __RS2_MATH1(atanh)
__RS2_MATH1(cosh) __RS2_MATH1(sinh) __RS2_MATH1(tanh)
__RS2_MATH1(exp) __RS2_MATH1(exp2) __RS2_MATH1(expm1)
__RS2_MATH1(log) __RS2_MATH1(log10) __RS2_MATH1(log1p) __RS2_MATH1(log2) __RS2_MATH1(logb)
__RS2_MATH1(cbrt) __RS2_MATH1(fabs) __RS2_MATH2(hypot) __RS2_MATH2(pow) __RS2_MATH1(sqrt)
__RS2_MATH1(erf) __RS2_MATH1(erfc) __RS2_MATH1(lgamma) __RS2_MATH1(tgamma)
__RS2_MATH1(ceil) __RS2_MATH1(floor) __RS2_MATH1(nearbyint) __RS2_MATH1(rint)
__RS2_MATH1(round) __RS2_MATH1(trunc)
__RS2_MATH2(fmod) __RS2_MATH2(remainder) __RS2_MATH2(copysign) __RS2_MATH2(nextafter)
__RS2_MATH2(fdim) __RS2_MATH2(fmax) __RS2_MATH2(fmin)
#undef __RS2_MATH1
#undef __RS2_MATH2
extern int signgam _RS2_DATA(signgam);
double fma(double, double, double);
float fmaf(float, float, float);
double frexp(double, int *);
float frexpf(float, int *);
long double frexpl(long double, int *);
double ldexp(double, int);
float ldexpf(float, int);
long double ldexpl(long double, int);
double scalbn(double, int);
float scalbnf(float, int);
double scalbln(double, long);
long double scalbnl(long double, int);
float scalblnf(float, long);
long double scalblnl(long double, long);
int ilogb(double);
int ilogbf(float);
int ilogbl(long double);
long double fmal(long double, long double, long double);
float nexttowardf(float, long double);
double nexttoward(double, long double);
long double nexttowardl(long double, long double);
long lrintf(float);
long lrintl(long double);
long long llrintf(float);
long long llrintl(long double);
long lroundf(float);
long lroundl(long double);
long long llroundf(float);
long long llroundl(long double);
float remquof(float, float, int *);
long double remquol(long double, long double, int *);
double modf(double, double *);
float modff(float, float *);
long double modfl(long double, long double *);
long lrint(double);
long long llrint(double);
long lround(double);
long long llround(double);
double remquo(double, double, int *);
double nan(const char *);
float nanf(const char *);
long double nanl(const char *);
_RS2_END
#endif
