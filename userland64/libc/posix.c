/* Locale handles, clocks and the POSIX file calls libc++ uses. */
#include "libc.h"
#include <locale.h>
#include <string.h>
#include <stdlib.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/stat.h>
#include <sys/statvfs.h>
#include <sys/time.h>

/* One locale, "C". */
struct _rs2_locale { int unused; };
static struct _rs2_locale c_locale;
locale_t newlocale(int mask, const char *name, locale_t base) {
    (void)mask;
    (void)base;
    if (name && (!*name || !strcmp(name, "C") || !strcmp(name, "POSIX")))
        return &c_locale;
    errno = ENOENT;
    return 0;
}
locale_t duplocale(locale_t l) { return l; }
void freelocale(locale_t l) { (void)l; }
locale_t uselocale(locale_t l) { (void)l; return &c_locale; }

int clock_gettime(clockid_t c, struct timespec *ts) {
    if (c == CLOCK_REALTIME)
        return timespec_get(ts, TIME_UTC) ? 0 : -1;
    if (c != CLOCK_MONOTONIC) {
        errno = EINVAL;
        return -1;
    }
    static ULONG freq;
    ULONGLONG t;
    if (!freq && DosTmrQueryFreq(&freq))
        freq = 0;
    if (!freq || DosTmrQueryTime(&t)) {
        ULONG ms = 0;
        DosQuerySysInfo(QSV_MS_COUNT, QSV_MS_COUNT, &ms, sizeof ms);
        ts->tv_sec = ms / 1000;
        ts->tv_nsec = ms % 1000 * 1000000L;
        return 0;
    }
    ts->tv_sec = (time_t)(t / freq);
    ts->tv_nsec = (long)(t % freq * 1000000000ULL / freq);
    return 0;
}
int clock_getres(clockid_t c, struct timespec *ts) {
    ts->tv_sec = 0;
    ts->tv_nsec = 1000;
    return 0;
}

/* Random bytes from the processor's generator. Without one (RDRAND),
 * from timers: not fit for keys. */
static int rdrand(unsigned long *v) {
    unsigned a = 1, b, c = 0, d;
    __asm__("cpuid" : "+a"(a), "=b"(b), "+c"(c), "=d"(d));
    if (!(c >> 30 & 1))
        return 0;
    for (int i = 0; i < 10; i++) {
        unsigned char ok;
        __asm__("rdrand %0; setc %1" : "=r"(*v), "=qm"(ok));
        if (ok)
            return 1;
    }
    return 0;
}
int getentropy(void *buf, size_t n) {
    if (n > 256) {
        errno = EIO;
        return -1;
    }
    static unsigned long pool;
    unsigned char *p = buf;
    while (n) {
        unsigned long v;
        if (!rdrand(&v)) {
            ULONGLONG t = 0;
            DosTmrQueryTime(&t);
            unsigned lo, hi;
            __asm__("rdtsc" : "=a"(lo), "=d"(hi));
            v = __atomic_add_fetch(&pool, t ^ (unsigned long)hi << 32 ^ lo ^ (uintptr_t)&v, __ATOMIC_RELAXED) +
                0x9e3779b97f4a7c15UL;
            v = (v ^ v >> 30) * 0xbf58476d1ce4e5b9UL;
            v = (v ^ v >> 27) * 0x94d049bb133111ebUL;
            v ^= v >> 31;
        }
        size_t k = n < sizeof v ? n : sizeof v;
        memcpy(p, &v, k);
        p += k;
        n -= k;
    }
    return 0;
}

int truncate(const char *path, off_t size) {
    int fd = open(path, O_RDWR);
    if (fd < 0)
        return -1;
    int r = ftruncate(fd, size);
    close(fd);
    return r;
}

static int none(int e) {
    errno = e;
    return -1;
}
int link(const char *a, const char *b) { (void)a; (void)b; return none(ENOSYS); }
int symlink(const char *a, const char *b) { (void)a; (void)b; return none(ENOSYS); }
ssize_t readlink(const char *__restrict p, char *__restrict b, size_t n) { (void)p; (void)b; (void)n; return none(EINVAL); }

static APIRET set_readonly(FILESTATUS3 *st, mode_t mode) {
    st->attrFile = mode & S_IWUSR ? st->attrFile & ~FILE_READONLY : st->attrFile | FILE_READONLY;
    st->attrFile &= FILE_READONLY | FILE_HIDDEN | FILE_SYSTEM | FILE_ARCHIVED;
    return 0;
}
int fchmod(int fd, mode_t mode) {
    FILESTATUS3 st;
    APIRET rc = DosQueryFileInfo((HFILE)fd, FIL_STANDARD, &st, sizeof st);
    if (!rc) {
        set_readonly(&st, mode);
        rc = DosSetFileInfo((HFILE)fd, FIL_STANDARD, &st, sizeof st);
    }
    return rc ? _rs2_oserr(rc) : 0;
}

int utimes(const char *path, const struct timeval tv[2]) {
    FILESTATUS3 st;
    APIRET rc = DosQueryPathInfo((PCSZ)path, FIL_STANDARD, &st, sizeof st);
    if (rc)
        return _rs2_oserr(rc);
    for (int i = 0; i < 2; i++) {
        struct tm tm;
        time_t t = tv ? tv[i].tv_sec : time(0);
        localtime_r(&t, &tm);
        if (tm.tm_year < 80 || tm.tm_year > 207)
            return none(EINVAL);
        FDATE d = {(USHORT)((tm.tm_year - 80) << 9 | (tm.tm_mon + 1) << 5 | tm.tm_mday)};
        FTIME m = {(USHORT)(tm.tm_hour << 11 | tm.tm_min << 5 | tm.tm_sec / 2)};
        if (i == 0)
            st.fdateLastAccess = d, st.ftimeLastAccess = m;
        else
            st.fdateLastWrite = d, st.ftimeLastWrite = m;
    }
    st.attrFile &= FILE_READONLY | FILE_HIDDEN | FILE_SYSTEM | FILE_ARCHIVED;
    rc = DosSetPathInfo((PCSZ)path, FIL_STANDARD, &st, sizeof st, 0);
    return rc ? _rs2_oserr(rc) : 0;
}

int statvfs(const char *__restrict path, struct statvfs *__restrict s) {
    ULONG drive = path[0] && path[1] == ':' ? (ULONG)((path[0] | 0x20) - 'a' + 1) : 0;
    FSALLOCATE a;
    APIRET rc = DosQueryFSInfo(drive, 1 /* FSIL_ALLOC */, &a, sizeof a);
    if (rc)
        return _rs2_oserr(rc);
    memset(s, 0, sizeof *s);
    s->f_bsize = s->f_frsize = (unsigned long)a.cbSector * a.cSectorUnit;
    s->f_blocks = a.cUnit;
    s->f_bfree = s->f_bavail = a.cUnitAvail;
    s->f_namemax = 255;
    return 0;
}

/* A full path: _fullpath does not need the file, realpath does. */
static char *full(const char *path, char *buf, size_t size) {
    char tmp[600], *out = tmp;
    if (strlen(path) > 259) {
        errno = ENAMETOOLONG;
        return 0;
    }
    int drive = path[0] && path[1] == ':';
    char d = drive ? (char)(path[0] & ~0x20) : 0;
    const char *rest = path + (drive ? 2 : 0);
    char cwd[300];
    if (!(rest[0] == '\\' || rest[0] == '/')) {
        extern char *_getdcwd(int, char *, size_t);
        if (!_getdcwd(drive ? d - 'A' + 1 : 0, cwd, sizeof cwd))
            return 0;
        out = stpcpy(tmp, cwd);
    } else {
        extern int _getdrive(void);
        *out++ = drive ? d : (char)('A' + _getdrive() - 1);
        *out++ = ':';
    }
    *out++ = '\\';
    strcpy(out, rest);
    /* Drop ".", "..", double and trailing separators. */
    char *w = tmp + 2, *r = tmp + 2;
    while (*r) {
        while (*r == '\\' || *r == '/')
            r++;
        size_t n = strcspn(r, "\\/");
        if (n == 2 && r[0] == '.' && r[1] == '.') {
            while (w > tmp + 2 && *--w != '\\')
                ;
        } else if (n && !(n == 1 && *r == '.')) {
            *w++ = '\\';
            memmove(w, r, n);
            w += n;
        }
        r += n;
    }
    if (w == tmp + 2)
        *w++ = '\\';
    *w = 0;
    size_t n = strlen(tmp) + 1;
    if (!buf)
        return strdup(tmp);
    if (n > size) {
        errno = ERANGE;
        return 0;
    }
    return memcpy(buf, tmp, n);
}
char *_fullpath(char *buf, const char *path, size_t size) { return full(path, buf, size); }
char *realpath(const char *__restrict path, char *__restrict buf) {
    if (access(path, F_OK))
        return 0;
    return full(path, buf, 261);
}
