#ifndef _STDDEF_H
#define _STDDEF_H
#include "_rs2libc.h"
#ifndef _PTRDIFF_T_DEFINED
#define _PTRDIFF_T_DEFINED
typedef __PTRDIFF_TYPE__ ptrdiff_t;
#endif
typedef struct {
    long long __ll __attribute__((aligned(__alignof__(long long))));
    long double __ld __attribute__((aligned(__alignof__(long double))));
} max_align_t;
#define offsetof(t, m) __builtin_offsetof(t, m)
#endif
