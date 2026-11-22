/* A C DLL's own start-up code, linked into every DLL cc64.sh makes. The
 * kernel calls _DLL_InitTerm(hmod, flag) once per process (docs/LIBC64.md):
 * flag 0 runs the DLL's constructors and puts its destructors on the
 * C library's exit list, so that exit() runs them while stdio still
 * works; flag 1 (DosFreeModule, or the end of a process that did not go
 * through exit) runs them if they have not run yet. */
#include "libc.h"

int __cxa_atexit(void (*fn)(void *), void *arg, void *dso);
void __cxa_finalize(void *dso);

extern void (*__init_array_start[])(void), (*__init_array_end[])(void);
extern void (*__fini_array_start[])(void), (*__fini_array_end[])(void);

/* This module, for __cxa_atexit (and C++). */
HIDDEN void *__dso_handle = &__dso_handle;
/* This DLL's module handle. */
HIDDEN unsigned long _rs2_dll_hmod;

static void run_fini(void *arg) {
    (void)arg;
    for (void (**f)(void) = __fini_array_end; f > __fini_array_start;)
        (*--f)();
}

__attribute__((visibility("default"))) unsigned long _DLL_InitTerm(unsigned long hmod, unsigned long flag) {
    if (flag == 0) {
        _rs2_dll_hmod = hmod;
        for (void (**f)(void) = __init_array_start; f < __init_array_end; f++)
            (*f)();
        __cxa_atexit(run_fini, 0, &__dso_handle);
    } else {
        __cxa_finalize(&__dso_handle);
    }
    return 1;
}
