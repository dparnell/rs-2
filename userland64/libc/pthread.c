/* POSIX threads, thread-exit functions and thread_local (docs/LIBC64.md).
 * Everything waits with __rs2_wait, a futex: waiters queue by address and
 * each sleeps on its thread's own event semaphore. */
#include "libc.h"
#include <pthread.h>
#include <process.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

/* ---- wait and wake ---- */

struct waiter {
    const volatile void *addr;
    HEV ev;
    int woken;
    struct waiter *next;
};
static struct bucket {
    volatile int lock;
    struct waiter *head;
} buckets[64];

static struct bucket *bucket_of(const volatile void *a) {
    uintptr_t h = (uintptr_t)a >> 2;
    h ^= h >> 6 ^ h >> 12 ^ h >> 20;
    return &buckets[h & 63];
}

static void dequeue(struct bucket *b, struct waiter *w) {
    for (struct waiter **p = &b->head; *p; p = &(*p)->next)
        if (*p == w) {
            *p = w->next;
            return;
        }
}


int __rs2_wait(const volatile void *addr, unsigned long long expected, int size, long ms) {
    struct _rs2_thread *t = _rs2_self();
    if (!t->event) {
        HEV ev = 0;
        if (DosCreateEventSem(0, &ev, 0, 0)) {
            DosSleep(1);   /* out of semaphores: poll */
            return 0;
        }
        t->event = ev;
    }
    ULONG posted;
    DosResetEventSem(t->event, &posted);
    struct waiter w = {addr, t->event, 0, 0};
    struct bucket *b = bucket_of(addr);
    _rs2_spin_lock(&b->lock);
    unsigned long long v = size == 8 ? __atomic_load_n((const volatile unsigned long long *)addr, __ATOMIC_SEQ_CST)
                                     : __atomic_load_n((const volatile unsigned *)addr, __ATOMIC_SEQ_CST);
    if (v != (size == 8 ? expected : (unsigned)expected)) {
        _rs2_spin_unlock(&b->lock);
        return EAGAIN;
    }
    struct waiter **p = &b->head;   /* first come, first woken */
    while (*p)
        p = &(*p)->next;
    *p = &w;
    _rs2_spin_unlock(&b->lock);
    APIRET rc = DosWaitEventSem(w.ev, ms < 0 ? (ULONG)-1 : ms > 0x7ffffffe ? 0x7ffffffe : (ULONG)ms);
    /* The waker is done with w once it lets go of the lock. */
    _rs2_spin_lock(&b->lock);
    if (!w.woken)
        dequeue(b, &w);
    _rs2_spin_unlock(&b->lock);
    return w.woken || rc != 640 /* ERROR_TIMEOUT */ ? 0 : ETIMEDOUT;
}

int __rs2_wake(const volatile void *addr, int n) {
    struct bucket *b = bucket_of(addr);
    int woke = 0;
    _rs2_spin_lock(&b->lock);
    for (struct waiter **p = &b->head; *p && woke < n;) {
        struct waiter *w = *p;
        if (w->addr != addr) {
            p = &w->next;
            continue;
        }
        *p = w->next;
        w->woken = 1;
        DosPostEventSem(w->ev);
        woke++;
    }
    _rs2_spin_unlock(&b->lock);
    return woke;
}

/* ---- what runs when a thread ends ---- */

struct tdtor {
    struct tdtor *next;
    void (*fn)(void *);
    void *arg;
};

int __cxa_thread_atexit(void (*fn)(void *), void *arg, void *dso) {
    (void)dso;
    struct tdtor *d = malloc(sizeof *d);
    if (!d)
        return -1;
    struct _rs2_thread *t = _rs2_self();
    *d = (struct tdtor){t->dtors, fn, arg};
    t->dtors = d;
    return 0;
}

void _rs2_thread_dtors(void) {
    struct _rs2_thread *t = _rs2_self();
    while (t->dtors) {
        struct tdtor *d = t->dtors;
        t->dtors = d->next;
        d->fn(d->arg);
        free(d);
    }
}

static struct key {
    int used;
    void (*dtor)(void *);
} keys[PTHREAD_KEYS_MAX];
static volatile int keys_lock;

static void key_dtors(void) {
    struct _rs2_thread *t = _rs2_self();
    for (int again = 1, round = 0; again && round < 4 && t->specific; round++) {
        again = 0;
        for (int i = 0; i < PTHREAD_KEYS_MAX; i++) {
            void *v = t->specific[i];
            t->specific[i] = 0;
            if (v && keys[i].used && keys[i].dtor) {
                keys[i].dtor(v);
                again = 1;
            }
        }
    }
    free(t->specific);
    t->specific = 0;
}

/* ---- thread_local (emulated TLS: clang -femulated-tls) ---- */

struct emutls {
    size_t size, align;
    uintptr_t index;   /* 1, 2, ... once used */
    const void *init;
};
static uintptr_t emutls_count;

void *__emutls_get_address(struct emutls *c) {
    uintptr_t i = __atomic_load_n(&c->index, __ATOMIC_ACQUIRE);
    if (!i) {
        uintptr_t fresh = __atomic_add_fetch(&emutls_count, 1, __ATOMIC_RELAXED);
        i = 0;
        if (__atomic_compare_exchange_n(&c->index, &i, fresh, 0, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE))
            i = fresh;
    }
    struct _rs2_thread *t = _rs2_self();
    if (i > t->tls_count) {
        size_t n = (i + 31) & ~31UL;
        void **v = realloc(t->tls, n * sizeof *v);
        if (!v)
            abort();
        memset(v + t->tls_count, 0, (n - t->tls_count) * sizeof *v);
        t->tls = v;
        t->tls_count = n;
    }
    if (!t->tls[i - 1]) {
        void *p = aligned_alloc(c->align < 16 ? 16 : c->align, c->size);
        if (!p)
            abort();
        if (c->init)
            memcpy(p, c->init, c->size);
        else
            memset(p, 0, c->size);
        t->tls[i - 1] = p;
    }
    return t->tls[i - 1];
}

void _rs2_thread_exit(void) {
    struct _rs2_thread *t = _rs2_self();
    _rs2_thread_dtors();
    key_dtors();
    for (size_t i = 0; i < t->tls_count; i++)
        free(t->tls[i]);
    free(t->tls);
    t->tls = 0;
    t->tls_count = 0;
    _rs2_thread_done();
}

/* ---- threads ---- */

struct _rs2_pthread {
    TID tid;
    void *(*fn)(void *);
    void *arg, *result;
    int done;        /* the thread function has returned */
    int owners;      /* the thread, and whoever may join it */
};

static void release(pthread_t p) {
    if (__atomic_sub_fetch(&p->owners, 1, __ATOMIC_ACQ_REL) == 0)
        free(p);
}

pthread_t pthread_self(void) {
    struct _rs2_thread *t = _rs2_self();
    if (!t->pthread) {
        /* The first thread, or one pthread_create did not start. */
        pthread_t p = calloc(1, sizeof *p);
        PTIB tib;
        PPIB pib;
        if (!p)
            abort();
        DosGetInfoBlocks(&tib, &pib);
        p->tid = tib->tib_ptib2->tib2_ultid;
        p->owners = 2;
        t->pthread = p;
    }
    return t->pthread;
}

int pthread_equal(pthread_t a, pthread_t b) { return a == b; }

void pthread_exit(void *result) {
    pthread_t p = pthread_self();
    p->result = result;
    _rs2_thread_exit();
    /* From here on, nothing that needs the thread's state. A joiner may
     * free p as soon as done is set, so the waker must not touch it. */
    __atomic_store_n(&p->done, 1, __ATOMIC_SEQ_CST);
    __rs2_wake(&p->done, 0x7fffffff);
    release(p);
    DosExit(EXIT_THREAD, 0);
    __builtin_unreachable();
}

static void thread_main(ULONG_PTR arg) {
    pthread_t p = (pthread_t)arg;
    _rs2_self()->pthread = p;
    pthread_exit(p->fn(p->arg));
}

int pthread_create(pthread_t *out, const pthread_attr_t *a, void *(*fn)(void *), void *arg) {
    pthread_t p = calloc(1, sizeof *p);
    if (!p)
        return EAGAIN;
    p->fn = fn;
    p->arg = arg;
    int detached = a && a->detached;
    p->owners = detached ? 2 : 3;   /* the caller holds one until it is done */
    size_t stack = a && a->stack ? a->stack : 1 << 20;
    TID tid;
    APIRET rc = DosCreateThread(&tid, thread_main, (ULONG_PTR)p, CREATE_READY | STACK_SPARSE, (ULONG)stack);
    if (rc) {
        free(p);
        return EAGAIN;
    }
    p->tid = tid;
    *out = p;
    release(p);
    return 0;
}

int pthread_join(pthread_t p, void **result) {
    if (p == pthread_self())
        return EDEADLK;
    while (!__atomic_load_n(&p->done, __ATOMIC_SEQ_CST))
        __rs2_wait(&p->done, 0, 4, -1);
    if (result)
        *result = p->result;
    release(p);
    return 0;
}

int pthread_detach(pthread_t p) {
    release(p);
    return 0;
}

int sched_yield(void) {
    DosSleep(0);
    return 0;
}

int pthread_attr_init(pthread_attr_t *a) {
    a->stack = 0;
    a->detached = 0;
    return 0;
}
int pthread_attr_destroy(pthread_attr_t *a) { (void)a; return 0; }
int pthread_attr_setstacksize(pthread_attr_t *a, size_t n) {
    if (n < PTHREAD_STACK_MIN || n > 0x10000000)
        return EINVAL;
    a->stack = n;
    return 0;
}
int pthread_attr_getstacksize(const pthread_attr_t *a, size_t *n) {
    *n = a->stack ? a->stack : 1 << 20;
    return 0;
}
int pthread_attr_setdetachstate(pthread_attr_t *a, int d) {
    a->detached = d;
    return 0;
}

/* ---- mutexes: state 0 free, 1 held, 2 held with waiters ---- */

int pthread_mutexattr_init(pthread_mutexattr_t *a) { a->type = 0; return 0; }
int pthread_mutexattr_destroy(pthread_mutexattr_t *a) { (void)a; return 0; }
int pthread_mutexattr_settype(pthread_mutexattr_t *a, int t) {
    if (t < 0 || t > PTHREAD_MUTEX_ERRORCHECK)
        return EINVAL;
    a->type = t;
    return 0;
}

int pthread_mutex_init(pthread_mutex_t *m, const pthread_mutexattr_t *a) {
    *m = (pthread_mutex_t)PTHREAD_MUTEX_INITIALIZER;
    if (a)
        m->type = a->type;
    return 0;
}
int pthread_mutex_destroy(pthread_mutex_t *m) { return m->state ? EBUSY : 0; }

static int take(int *state) {
    int zero = 0;
    return __atomic_compare_exchange_n(state, &zero, 1, 0, __ATOMIC_ACQUIRE, __ATOMIC_RELAXED);
}

int pthread_mutex_trylock(pthread_mutex_t *m) {
    if (m->type) {
        void *me = _rs2_self();
        if (__atomic_load_n(&m->owner, __ATOMIC_RELAXED) == me) {
            if (m->type != PTHREAD_MUTEX_RECURSIVE)
                return EBUSY;
            m->count++;
            return 0;
        }
        if (!take(&m->state))
            return EBUSY;
        __atomic_store_n(&m->owner, me, __ATOMIC_RELAXED);
        m->count = 1;
        return 0;
    }
    return take(&m->state) ? 0 : EBUSY;
}

int pthread_mutex_lock(pthread_mutex_t *m) {
    int r = pthread_mutex_trylock(m);
    if (r == 0)
        return 0;
    if (m->type == PTHREAD_MUTEX_ERRORCHECK && m->owner == _rs2_self())
        return EDEADLK;
    for (int i = 0; i < 100; i++) {
        __builtin_ia32_pause();
        if (!__atomic_load_n(&m->state, __ATOMIC_RELAXED) && pthread_mutex_trylock(m) == 0)
            return 0;
    }
    while (__atomic_exchange_n(&m->state, 2, __ATOMIC_ACQUIRE))
        __rs2_wait(&m->state, 2, 4, -1);
    if (m->type) {
        __atomic_store_n(&m->owner, _rs2_self(), __ATOMIC_RELAXED);
        m->count = 1;
    }
    return 0;
}

int pthread_mutex_unlock(pthread_mutex_t *m) {
    if (m->type) {
        if (m->owner != _rs2_self())
            return EPERM;
        if (--m->count)
            return 0;
        __atomic_store_n(&m->owner, 0, __ATOMIC_RELAXED);
    }
    if (__atomic_exchange_n(&m->state, 0, __ATOMIC_RELEASE) == 2)
        __rs2_wake(&m->state, 1);
    return 0;
}

/* ---- condition variables ---- */

int pthread_condattr_init(pthread_condattr_t *a) { a->clock = CLOCK_REALTIME; return 0; }
int pthread_condattr_destroy(pthread_condattr_t *a) { (void)a; return 0; }
int pthread_condattr_setclock(pthread_condattr_t *a, clockid_t c) { a->clock = c; return 0; }
int pthread_cond_init(pthread_cond_t *c, const pthread_condattr_t *a) {
    c->seq = 0;
    c->clock = a ? a->clock : CLOCK_REALTIME;
    return 0;
}
int pthread_cond_destroy(pthread_cond_t *c) { (void)c; return 0; }

static int cond_wait(pthread_cond_t *c, pthread_mutex_t *m, long ms) {
    int seq = __atomic_load_n(&c->seq, __ATOMIC_SEQ_CST);
    int count = m->count;
    if (m->type == PTHREAD_MUTEX_RECURSIVE)
        m->count = 1;
    int r = pthread_mutex_unlock(m);
    if (r)
        return r;
    r = __rs2_wait(&c->seq, (unsigned)seq, 4, ms);
    pthread_mutex_lock(m);
    if (m->type == PTHREAD_MUTEX_RECURSIVE)
        m->count = count;
    return r == ETIMEDOUT ? ETIMEDOUT : 0;
}

int pthread_cond_wait(pthread_cond_t *c, pthread_mutex_t *m) { return cond_wait(c, m, -1); }

int pthread_cond_timedwait(pthread_cond_t *c, pthread_mutex_t *m, const struct timespec *until) {
    if (until->tv_nsec < 0 || until->tv_nsec >= 1000000000)
        return EINVAL;
    for (;;) {
        struct timespec now;
        clock_gettime(c->clock, &now);
        long s = until->tv_sec - now.tv_sec;
        long ms = s > 1000000 ? 1000000000L : s * 1000 + (until->tv_nsec - now.tv_nsec + 999999) / 1000000;
        if (ms <= 0)
            return ETIMEDOUT;
        int r = cond_wait(c, m, ms);
        if (r != ETIMEDOUT)
            return r;
    }
}

int pthread_cond_signal(pthread_cond_t *c) {
    __atomic_add_fetch(&c->seq, 1, __ATOMIC_SEQ_CST);
    __rs2_wake(&c->seq, 1);
    return 0;
}
int pthread_cond_broadcast(pthread_cond_t *c) {
    __atomic_add_fetch(&c->seq, 1, __ATOMIC_SEQ_CST);
    __rs2_wake(&c->seq, 0x7fffffff);
    return 0;
}

/* ---- read-write locks: state = readers, or -1 for a writer ---- */

int pthread_rwlock_init(pthread_rwlock_t *l, const pthread_rwlockattr_t *a) {
    (void)a;
    l->state = l->waiters = 0;
    return 0;
}
int pthread_rwlock_destroy(pthread_rwlock_t *l) { return l->state ? EBUSY : 0; }
int pthread_rwlock_tryrdlock(pthread_rwlock_t *l) {
    int s = __atomic_load_n(&l->state, __ATOMIC_RELAXED);
    while (s >= 0)
        if (__atomic_compare_exchange_n(&l->state, &s, s + 1, 0, __ATOMIC_ACQUIRE, __ATOMIC_RELAXED))
            return 0;
    return EBUSY;
}
int pthread_rwlock_trywrlock(pthread_rwlock_t *l) {
    int s = 0;
    return __atomic_compare_exchange_n(&l->state, &s, -1, 0, __ATOMIC_ACQUIRE, __ATOMIC_RELAXED) ? 0 : EBUSY;
}
static int rw_lock(pthread_rwlock_t *l, int (*try)(pthread_rwlock_t *)) {
    while (try(l)) {
        __atomic_add_fetch(&l->waiters, 1, __ATOMIC_SEQ_CST);
        int s = __atomic_load_n(&l->state, __ATOMIC_SEQ_CST);
        if (s)
            __rs2_wait(&l->state, (unsigned)s, 4, -1);
        __atomic_sub_fetch(&l->waiters, 1, __ATOMIC_SEQ_CST);
    }
    return 0;
}
int pthread_rwlock_rdlock(pthread_rwlock_t *l) { return rw_lock(l, pthread_rwlock_tryrdlock); }
int pthread_rwlock_wrlock(pthread_rwlock_t *l) { return rw_lock(l, pthread_rwlock_trywrlock); }
int pthread_rwlock_unlock(pthread_rwlock_t *l) {
    int s = __atomic_load_n(&l->state, __ATOMIC_RELAXED);
    if (s == 0)
        return EPERM;
    if (s < 0) {
        __atomic_store_n(&l->state, 0, __ATOMIC_SEQ_CST);
        s = 0;
    } else {
        s = __atomic_sub_fetch(&l->state, 1, __ATOMIC_SEQ_CST);
    }
    if (__atomic_load_n(&l->waiters, __ATOMIC_SEQ_CST))
        __rs2_wake(&l->state, 0x7fffffff);
    return 0;
}

/* ---- once, keys ---- */

int pthread_once(pthread_once_t *o, void (*fn)(void)) {
    for (;;) {
        int s = __atomic_load_n(&o->state, __ATOMIC_ACQUIRE);
        if (s == 2)
            return 0;
        if (s == 0 && take(&o->state)) {
            fn();
            __atomic_store_n(&o->state, 2, __ATOMIC_SEQ_CST);
            __rs2_wake(&o->state, 0x7fffffff);
            return 0;
        }
        if (s == 1)
            __rs2_wait(&o->state, 1, 4, -1);
    }
}

int pthread_key_create(pthread_key_t *k, void (*dtor)(void *)) {
    _rs2_spin_lock(&keys_lock);
    for (int i = 0; i < PTHREAD_KEYS_MAX; i++)
        if (!keys[i].used) {
            keys[i].used = 1;
            keys[i].dtor = dtor;
            _rs2_spin_unlock(&keys_lock);
            *k = (pthread_key_t)i;
            return 0;
        }
    _rs2_spin_unlock(&keys_lock);
    return EAGAIN;
}

int pthread_key_delete(pthread_key_t k) {
    if (k >= PTHREAD_KEYS_MAX || !keys[k].used)
        return EINVAL;
    keys[k].used = 0;
    return 0;
}

void *pthread_getspecific(pthread_key_t k) {
    struct _rs2_thread *t = _rs2_self();
    return k < PTHREAD_KEYS_MAX && t->specific ? t->specific[k] : 0;
}

int pthread_setspecific(pthread_key_t k, const void *v) {
    struct _rs2_thread *t = _rs2_self();
    if (k >= PTHREAD_KEYS_MAX || !keys[k].used)
        return EINVAL;
    if (!t->specific && !(t->specific = calloc(PTHREAD_KEYS_MAX, sizeof(void *))))
        return ENOMEM;
    t->specific[k] = (void *)v;
    return 0;
}

long sysconf(int name) {
    ULONG v = 0;
    switch (name) {
    case _SC_PAGESIZE: return 4096;
    case _SC_NPROCESSORS_ONLN:
        DosQuerySysInfo(QSV_NUMPROCESSORS, QSV_NUMPROCESSORS, &v, sizeof v);
        return v ? v : 1;
    }
    errno = EINVAL;
    return -1;
}
