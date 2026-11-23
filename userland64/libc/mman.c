/* mmap over DosAllocMem: memory above 4 GB (DosRead and DosWrite take it).
 * A file's contents are read in; a shared writable mapping is written back
 * when it is synchronised or unmapped. */
#include "libc.h"
#include <sys/mman.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

struct map {
    struct map *next;
    char *addr;
    size_t len;
    int fd;
    off_t off;
};
static struct map *maps;
static volatile int lock;

void *mmap(void *hint, size_t len, int prot, int flags, int fd, off_t off) {
    (void)hint;
    if (!len || len > 0xfffff000UL || flags & MAP_FIXED) {
        errno = len > 0xfffff000UL ? ENOMEM : EINVAL;
        return MAP_FAILED;
    }
    PVOID p = 0;
    ULONG cb = (ULONG)((len + 4095) & ~4095UL);
    ULONG fl = PAG_READ | PAG_WRITE | PAG_COMMIT | (prot & PROT_EXEC ? PAG_EXECUTE : 0);
    APIRET rc = DosAllocMem(&p, cb, fl | OBJ_ANY);
    if (rc)
        rc = DosAllocMem(&p, cb, fl);
    if (rc) {
        _rs2_oserr(rc);
        return MAP_FAILED;
    }
    if (!(flags & MAP_ANONYMOUS)) {
        char *d = p;
        size_t n = 0;
        while (n < len) {
            ssize_t k = pread(fd, d + n, len - n, off + (off_t)n);
            if (k < 0) {
                DosFreeMem(p);
                return MAP_FAILED;
            }
            if (!k)
                break;
            n += (size_t)k;
        }
        if (flags & MAP_SHARED && prot & PROT_WRITE) {
            struct map *m = malloc(sizeof *m);
            int h = dup(fd);
            if (!m || h < 0) {
                free(m);
                DosFreeMem(p);
                return MAP_FAILED;
            }
            *m = (struct map){0, d, len, h, off};
            _rs2_spin_lock(&lock);
            m->next = maps;
            maps = m;
            _rs2_spin_unlock(&lock);
        }
    }
    return p;
}

static int write_back(struct map *m) {
    size_t n = 0;
    while (n < m->len) {
        size_t k = m->len - n > 1 << 20 ? 1 << 20 : m->len - n;
        if (pwrite(m->fd, m->addr + n, k, m->off + (off_t)n) != (ssize_t)k)
            return -1;
        n += k;
    }
    return 0;
}

int msync(void *addr, size_t len, int flags) {
    (void)len;
    (void)flags;
    struct map *m;
    _rs2_spin_lock(&lock);
    for (m = maps; m && m->addr != addr; m = m->next)
        ;
    _rs2_spin_unlock(&lock);
    return m ? write_back(m) : 0;
}

int munmap(void *addr, size_t len) {
    (void)len;
    int r = 0;
    _rs2_spin_lock(&lock);
    struct map **pp = &maps, *m;
    while (*pp && (*pp)->addr != addr)
        pp = &(*pp)->next;
    m = *pp;
    if (m)
        *pp = m->next;
    _rs2_spin_unlock(&lock);
    if (m) {
        r = write_back(m);
        close(m->fd);
        free(m);
    }
    APIRET rc = DosFreeMem(addr);
    return rc ? _rs2_oserr(rc) : r;
}

int mprotect(void *addr, size_t len, int prot) {
    ULONG fl = PAG_READ | (prot & PROT_WRITE ? PAG_WRITE : 0) | (prot & PROT_EXEC ? PAG_EXECUTE : 0);
    APIRET rc = DosSetMem(addr, (ULONG)len, fl);
    return rc ? _rs2_oserr(rc) : 0;
}
int madvise(void *a, size_t n, int v) { (void)a; (void)n; (void)v; return 0; }
int posix_madvise(void *a, size_t n, int v) { (void)a; (void)n; (void)v; return 0; }
