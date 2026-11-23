/* The heap: boundary-tagged chunks in regions from DosAllocMem, free
 * chunks in size-class bins, neighbours merged on free. Large blocks get
 * regions of their own. The first LOW_MAX bytes of regions are below
 * 4 GB, so that a program of ordinary size can pass any pointer from
 * malloc to any OS/2 call; after that, and for blocks of HIGH_MIN bytes
 * and more, memory comes from above 4 GB. One lock for all threads. */
#include "libc.h"
#include <stdlib.h>
#include <string.h>
#include <malloc.h>

/* A chunk: [size_t prev_size][size_t size|flags] then the user's memory.
 * prev_size is valid only while the previous chunk is free. */
#define INUSE 1UL
#define PREV_INUSE 2UL
#define DIRECT 4UL
#define FLAGS 7UL
#define HDR 16UL
#define ALIGN 16UL
#define MIN_CHUNK 32UL
#define REGION (4UL << 20)
#define DIRECT_MIN (1UL << 20)
#define LOW_MAX (96UL << 20)
#define HIGH_REGION (16UL << 20)
#define HIGH_MIN (16UL << 20)
#define NBINS 64

typedef struct chunk {
    size_t prev_size;
    size_t size;
    struct chunk *next, *prev;   /* while free */
} chunk;

static chunk bins[NBINS];        /* list heads (circular) */
static volatile int heap_lock;
static int heap_ready;

#define CSIZE(c) ((c)->size & ~FLAGS)
#define NEXT(c) ((chunk *)((char *)(c) + CSIZE(c)))
#define MEM(c) ((void *)((char *)(c) + HDR))
#define CHUNK(p) ((chunk *)((char *)(p) - HDR))

static int bin_of(size_t sz) {
    /* 32..1024 in steps of 32 (bins 0-31), then by powers of two. */
    if (sz <= 1024)
        return (int)(sz >> 5) - 1;
    int b = 31;
    size_t s = 1024;
    while (s < sz && b < NBINS - 1) {
        s <<= 1;
        b++;
    }
    return b;
}

static void unlink_chunk(chunk *c) {
    c->prev->next = c->next;
    c->next->prev = c->prev;
}

static void link_chunk(chunk *c) {
    chunk *h = &bins[bin_of(CSIZE(c))];
    c->next = h->next;
    c->prev = h;
    h->next->prev = c;
    h->next = c;
}

static void init(void) {
    for (int i = 0; i < NBINS; i++)
        bins[i].next = bins[i].prev = &bins[i];
    heap_ready = 1;
}

/* Marks c free (size sz), sets the footer and the next chunk's flag. */
static void set_free(chunk *c, size_t sz) {
    c->size = sz | (c->size & PREV_INUSE);
    chunk *n = NEXT(c);
    n->prev_size = sz;
    n->size &= ~PREV_INUSE;
}

/* Memory from the system, where it is preferred or else on the other side
 * of 4 GB (a system with little memory has no 64-bit arena). */
static void *system_alloc(size_t sz, int high) {
    PVOID p = 0;
    if (sz > 0xffff0000UL)
        return 0;
    for (int i = 0; i < 2; i++, high = !high)
        if (DosAllocMem(&p, (ULONG)sz, PAG_COMMIT | PAG_READ | PAG_WRITE | (high ? OBJ_ANY : 0)) == 0)
            return p;
    return 0;
}

static int grow(void) {   /* a request is below DIRECT_MIN */
    static size_t low;
    int high = low >= LOW_MAX;
    size_t sz = high ? HIGH_REGION : REGION;
    char *p = system_alloc(sz, high);
    if (!p)
        return 0;
    if (!((uintptr_t)p >> 32))
        low += sz;
    /* The region: one free chunk, then a 16-byte end marker in use. */
    chunk *c = (chunk *)p;
    c->size = PREV_INUSE;
    size_t csz = sz - HDR;
    chunk *end = (chunk *)((char *)p + csz);
    end->size = HDR | INUSE;
    set_free(c, csz);
    link_chunk(c);
    return 1;
}

static chunk *take(size_t sz) {
    for (int b = bin_of(sz); b < NBINS; b++) {
        chunk *h = &bins[b];
        for (chunk *c = h->next; c != h; c = c->next) {
            if (CSIZE(c) >= sz) {
                unlink_chunk(c);
                return c;
            }
        }
    }
    return 0;
}

/* Splits c (in use) so it keeps sz; the rest goes back as a free chunk. */
static void trim(chunk *c, size_t sz) {
    size_t have = CSIZE(c);
    if (have - sz < MIN_CHUNK)
        return;
    c->size = sz | (c->size & (PREV_INUSE | INUSE));
    chunk *r = NEXT(c);
    r->size = PREV_INUSE;
    set_free(r, have - sz);
    /* Merge with a free chunk after it. */
    chunk *n = NEXT(r);
    if (!(n->size & INUSE)) {
        unlink_chunk(n);
        set_free(r, CSIZE(r) + CSIZE(n));
    }
    link_chunk(r);
}

static size_t request(size_t n) {
    if (n > ((size_t)1 << 40))
        return 0;
    size_t sz = (n + HDR + ALIGN - 1) & ~(ALIGN - 1);
    return sz < MIN_CHUNK ? MIN_CHUNK : sz;
}

static void *direct(size_t sz) {
    size_t total = (sz + 0xfff) & ~0xfffUL;
    chunk *c = system_alloc(total, sz >= HIGH_MIN);
    if (!c)
        return 0;
    c->prev_size = 0;
    c->size = total | INUSE | DIRECT;
    return MEM(c);
}

void *_rs2_low_alloc(size_t n) {
    size_t total = (n + HDR + 0xfff) & ~0xfffUL;
    PVOID p = 0;
    if (n > 0xffff0000UL || DosAllocMem(&p, (ULONG)total, PAG_COMMIT | PAG_READ | PAG_WRITE) != 0) {
        errno = ENOMEM;
        return 0;
    }
    chunk *c = p;
    c->size = total | INUSE | DIRECT;
    return MEM(c);
}

void *malloc(size_t n) {
    size_t sz = request(n);
    if (!sz) {
        errno = ENOMEM;
        return 0;
    }
    if (sz >= DIRECT_MIN) {
        void *p = direct(sz);
        if (!p)
            errno = ENOMEM;
        return p;
    }
    _rs2_spin_lock(&heap_lock);
    if (!heap_ready)
        init();
    chunk *c = take(sz);
    if (!c && grow())
        c = take(sz);
    if (!c) {
        _rs2_spin_unlock(&heap_lock);
        errno = ENOMEM;
        return 0;
    }
    c->size |= INUSE;
    NEXT(c)->size |= PREV_INUSE;
    trim(c, sz);
    _rs2_spin_unlock(&heap_lock);
    return MEM(c);
}

void free(void *p) {
    if (!p)
        return;
    chunk *c = CHUNK(p);
    if (c->size & DIRECT) {
        /* prev_size: how far the region starts before (aligned_alloc). */
        DosFreeMem((char *)c - c->prev_size);
        return;
    }
    _rs2_spin_lock(&heap_lock);
    size_t sz = CSIZE(c);
    if (!(c->size & PREV_INUSE)) {
        chunk *pv = (chunk *)((char *)c - c->prev_size);
        unlink_chunk(pv);
        sz += CSIZE(pv);
        c = pv;
    }
    chunk *n = (chunk *)((char *)c + sz);
    if (!(n->size & INUSE)) {
        unlink_chunk(n);
        sz += CSIZE(n);
    }
    set_free(c, sz);
    link_chunk(c);
    _rs2_spin_unlock(&heap_lock);
}

size_t malloc_usable_size(void *p) {
    return p ? CSIZE(CHUNK(p)) - HDR : 0;
}

void *calloc(size_t n, size_t m) {
    size_t t;
    if (__builtin_mul_overflow(n, m, &t)) {
        errno = ENOMEM;
        return 0;
    }
    void *p = malloc(t);
    if (p && !(CHUNK(p)->size & DIRECT))   /* fresh regions are zero */
        memset(p, 0, t);
    return p;
}

void *realloc(void *p, size_t n) {
    if (!p)
        return malloc(n);
    if (n == 0) {
        free(p);
        return 0;
    }
    size_t sz = request(n);
    if (!sz) {
        errno = ENOMEM;
        return 0;
    }
    chunk *c = CHUNK(p);
    if (!(c->size & DIRECT)) {
        _rs2_spin_lock(&heap_lock);
        if (CSIZE(c) >= sz) {
            trim(c, sz);
            _rs2_spin_unlock(&heap_lock);
            return p;
        }
        chunk *nx = NEXT(c);
        if (!(nx->size & INUSE) && CSIZE(c) + CSIZE(nx) >= sz) {
            unlink_chunk(nx);
            c->size += CSIZE(nx);
            NEXT(c)->size |= PREV_INUSE;
            trim(c, sz);
            _rs2_spin_unlock(&heap_lock);
            return p;
        }
        _rs2_spin_unlock(&heap_lock);
    } else if (CSIZE(c) >= sz && CSIZE(c) / 2 < sz) {
        return p;
    }
    void *q = malloc(n);
    if (!q)
        return 0;
    size_t old = CSIZE(c) - HDR;
    memcpy(q, p, old < n ? old : n);
    free(p);
    return q;
}

void *reallocarray(void *p, size_t n, size_t m) {
    size_t t;
    if (__builtin_mul_overflow(n, m, &t)) {
        errno = ENOMEM;
        return 0;
    }
    return realloc(p, t);
}

void *aligned_alloc(size_t al, size_t n) {
    if (al <= ALIGN)
        return malloc(n);
    if (al & (al - 1)) {
        errno = EINVAL;
        return 0;
    }
    char *raw = malloc(n + al + MIN_CHUNK);
    if (!raw)
        return 0;
    chunk *c = CHUNK(raw);
    uintptr_t a = ((uintptr_t)raw + MIN_CHUNK + al - 1) & ~(al - 1);
    chunk *d = CHUNK((void *)a);
    if (c->size & DIRECT) {
        /* Keep the whole region: d points back at the region's start. */
        d->prev_size = (char *)d - (char *)c;
        d->size = (CSIZE(c) - d->prev_size) | INUSE | DIRECT;
        return (void *)a;
    }
    /* Give the space in front back as a free chunk. */
    _rs2_spin_lock(&heap_lock);
    size_t lead = (char *)d - (char *)c;
    size_t rest = CSIZE(c) - lead;
    d->size = rest | INUSE | PREV_INUSE;
    c->size = lead | (c->size & PREV_INUSE) | INUSE;
    _rs2_spin_unlock(&heap_lock);
    free(raw);
    return (void *)a;
}

int posix_memalign(void **out, size_t al, size_t n) {
    if (al < sizeof(void *) || (al & (al - 1)))
        return EINVAL;
    void *p = aligned_alloc(al, n);
    if (!p)
        return ENOMEM;
    *out = p;
    return 0;
}
