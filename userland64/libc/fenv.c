/* <fenv.h>: SSE (what float and double use) and x87 together. */
#include <fenv.h>

static unsigned get_mxcsr(void) { unsigned m; __asm__("stmxcsr %0" : "=m"(m)); return m; }
static void set_mxcsr(unsigned m) { __asm__("ldmxcsr %0" : : "m"(m)); }
static unsigned get_sw(void) { unsigned short w; __asm__("fnstsw %0" : "=a"(w)); return w; }
int fegetenv(fenv_t *e) {
    /* FNSTENV masks exceptions: load it back. */
    __asm__("fnstenv %0; fldenv %0" : "=m"(e->__x87));
    e->__mxcsr = get_mxcsr();
    return 0;
}
int fesetenv(const fenv_t *e) {
    if (e == FE_DFL_ENV) {
        __asm__("fninit");
        set_mxcsr(0x1f80);
        return 0;
    }
    __asm__("fldenv %0" : : "m"(e->__x87));
    set_mxcsr(e->__mxcsr);
    return 0;
}
int feclearexcept(int x) {
    fenv_t e;
    fegetenv(&e);
    e.__x87[1] &= ~(unsigned)(x & FE_ALL_EXCEPT);
    e.__mxcsr &= ~(unsigned)(x & FE_ALL_EXCEPT);
    return fesetenv(&e);
}
int fetestexcept(int x) { return (int)((get_mxcsr() | get_sw()) & (unsigned)x & FE_ALL_EXCEPT); }
int fegetexceptflag(fexcept_t *f, int x) { *f = (fexcept_t)fetestexcept(x); return 0; }
int fesetexceptflag(const fexcept_t *f, int x) {
    feclearexcept(x);
    set_mxcsr(get_mxcsr() | (*f & (unsigned)x & FE_ALL_EXCEPT));
    return 0;
}
int feraiseexcept(int x) { set_mxcsr(get_mxcsr() | ((unsigned)x & FE_ALL_EXCEPT)); return 0; }
int fegetround(void) { return (int)(get_mxcsr() >> 3 & 0xc00); }
int fesetround(int r) {
    if (r & ~0xc00)
        return -1;
    unsigned short cw;
    __asm__("fnstcw %0" : "=m"(cw));
    cw = (unsigned short)((cw & ~0xc00) | r);
    __asm__("fldcw %0" : : "m"(cw));
    set_mxcsr((get_mxcsr() & ~0x6000u) | (unsigned)r << 3);
    return 0;
}
int feholdexcept(fenv_t *e) {
    fegetenv(e);
    feclearexcept(FE_ALL_EXCEPT);
    set_mxcsr(get_mxcsr() | 0x1f80);
    return 0;
}
int feupdateenv(const fenv_t *e) {
    int x = fetestexcept(FE_ALL_EXCEPT);
    fesetenv(e);
    return feraiseexcept(x);
}
