/* The unwind tables of the process's 64-bit modules, for C++ exceptions
 * (docs/CXX64.md). Programs and DLLs announce theirs at start-up (crt1.c,
 * crtdll.c); the C library adds its own. libunwind asks by address. */
#include "libc.h"

static struct _rs2_frames *modules;
static volatile int lock;

void __rs2_add_frames(struct _rs2_frames *m) {
    _rs2_spin_lock(&lock);
    m->next = modules;
    modules = m;
    _rs2_spin_unlock(&lock);
}

void __rs2_remove_frames(struct _rs2_frames *m) {
    _rs2_spin_lock(&lock);
    for (struct _rs2_frames **p = &modules; *p; p = &(*p)->next)
        if (*p == m) {
            *p = m->next;
            break;
        }
    _rs2_spin_unlock(&lock);
}

/* The module `pc` is in: 1 and a copy of its record, or 0. */
int __rs2_find_frames(const void *pc, struct _rs2_frames *out) {
    static struct _rs2_frames own = RS2_FRAMES;
    static int added;
    if (!__atomic_exchange_n(&added, 1, __ATOMIC_ACQ_REL))
        __rs2_add_frames(&own);
    int found = 0;
    _rs2_spin_lock(&lock);
    for (struct _rs2_frames *m = modules; m; m = m->next)
        if ((const char *)pc >= m->text && (const char *)pc < m->text_end) {
            *out = *m;
            found = 1;
            break;
        }
    _rs2_spin_unlock(&lock);
    return found;
}
