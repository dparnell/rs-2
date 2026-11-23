/* A program's own start-up code, linked into every C program (the rest of
 * the C library may be in LIBC64.DLL): the loader calls
 * _start(hmod, 0, env, cmdline); the library takes it from there with
 * the program's main and constructor lists. */
#include "libc.h"

int main(int argc, char **argv, char **envp);

extern void (*__init_array_start[])(void), (*__init_array_end[])(void);
extern void (*__fini_array_start[])(void), (*__fini_array_end[])(void);

/* This module, for __cxa_atexit (and C++). */
HIDDEN void *__dso_handle = &__dso_handle;

static struct _rs2_frames frames = RS2_FRAMES;

unsigned long _start(ULONG hmod, ULONG reserved, PCSZ env, PCSZ cmdline) {
    (void)reserved;
    struct _rs2_startup st = {hmod, env, cmdline, main, __init_array_start, __init_array_end,
                              __fini_array_start, __fini_array_end};
    __rs2_add_frames(&frames);
    __rs2_start(&st);
}
