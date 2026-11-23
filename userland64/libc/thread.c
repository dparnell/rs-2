/* Per-thread state, locks and threads. */
#include "libc.h"
#include <process.h>
#include <stdlib.h>
#include <string.h>

/* One dword of thread-local memory holds each thread's block (blocks
 * are below 4 GB: DosAllocMem without OBJ_ANY). */
static ULONG *volatile tls_slot;
static volatile int tls_init;

static ULONG *slot(void) {
    ULONG *s = tls_slot;
    if (s)
        return s;
    _rs2_spin_lock(&tls_init);
    if (!tls_slot) {
        PULONG p = 0;
        if (DosAllocThreadLocalMemory(1, (PPVOID)&p) == 0)
            tls_slot = p;
    }
    _rs2_spin_unlock(&tls_init);
    return tls_slot;
}

/* Threads before the slot exists, or if it cannot be had, share this. */
static struct _rs2_thread fallback;

struct _rs2_thread *_rs2_self(void) {
    ULONG *s = slot();
    if (!s)
        return &fallback;
    struct _rs2_thread *t = (struct _rs2_thread *)(uintptr_t)*s;
    if (t)
        return t;
    PVOID p = 0;
    if (DosAllocMem(&p, sizeof(struct _rs2_thread), PAG_COMMIT | PAG_READ | PAG_WRITE) != 0)
        return &fallback;
    memset(p, 0, sizeof(struct _rs2_thread));
    *s = (ULONG)(uintptr_t)p;
    return p;
}

void _rs2_thread_done(void) {
    ULONG *s = tls_slot;
    if (s && *s) {
        PVOID p = (PVOID)(uintptr_t)*s;
        HEV ev = ((struct _rs2_thread *)p)->event;
        *s = 0;
        DosFreeMem(p);
        if (ev)
            DosCloseEventSem(ev);
    }
}

int *__errno(void) {
    return &_rs2_self()->err;
}

void _rs2_spin_lock(volatile int *l) {
    int spins = 0;
    while (__atomic_exchange_n(l, 1, __ATOMIC_ACQUIRE)) {
        while (*l) {
            if (++spins > 200) {
                DosSleep(0);
                spins = 0;
            }
            __builtin_ia32_pause();
        }
    }
}

void _rs2_spin_unlock(volatile int *l) {
    __atomic_store_n(l, 0, __ATOMIC_RELEASE);
}

void _rs2_lock(_rs2_lock_t *l) {
    struct _rs2_thread *me = _rs2_self();
    if (l->owner == me && l->locked) {
        l->count++;
        return;
    }
    _rs2_spin_lock(&l->locked);
    l->owner = me;
    l->count = 1;
}

void _rs2_unlock(_rs2_lock_t *l) {
    if (--l->count == 0) {
        l->owner = 0;
        _rs2_spin_unlock(&l->locked);
    }
}

struct start {
    void (*fn)(void *);
    void *arg;
};

static void thread_main(ULONG_PTR p) {
    struct start s = *(struct start *)p;
    free((void *)p);
    s.fn(s.arg);
    _endthread();
}

int _beginthread(void (*fn)(void *), void *stack, unsigned stack_size, void *arg) {
    (void)stack;
    struct start *s = malloc(sizeof *s);
    if (!s) {
        errno = ENOMEM;
        return -1;
    }
    s->fn = fn;
    s->arg = arg;
    if (stack_size < 65536)
        stack_size = 65536;
    TID tid;
    APIRET rc = DosCreateThread(&tid, thread_main, (ULONG_PTR)s, CREATE_READY | STACK_COMMITTED, stack_size);
    if (rc) {
        free(s);
        return _rs2_oserr(rc);
    }
    return (int)tid;
}

void _endthread(void) {
    _rs2_thread_exit();
    DosExit(EXIT_THREAD, 0);
    __builtin_unreachable();
}
