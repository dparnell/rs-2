/* <stdlib.h> number conversions, sorting, random numbers; <inttypes.h>,
 * <locale.h>, getopt. */
#include "libc.h"
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <limits.h>
#include <inttypes.h>
#include <locale.h>
#include <stdio.h>
#include <unistd.h>

/* Reads an unsigned number; *neg tells whether a '-' came first. Sets
 * *over on overflow of `max` (the magnitude limit). */
static unsigned long long scan(const char *s, char **end, int base, unsigned long long max, int *neg, int *over) {
    const char *p = s;
    *neg = 0;
    *over = 0;
    while (isspace((unsigned char)*p))
        p++;
    if (*p == '+' || *p == '-')
        *neg = *p++ == '-';
    if ((base == 0 || base == 16) && p[0] == '0' && (p[1] | 32) == 'x' && isxdigit((unsigned char)p[2])) {
        p += 2;
        base = 16;
    } else if (base == 0) {
        base = *p == '0' ? 8 : 10;
    }
    if (base < 2 || base > 36) {
        errno = EINVAL;
        if (end)
            *end = (char *)s;
        return 0;
    }
    unsigned long long v = 0;
    const char *start = p;
    for (;; p++) {
        int d;
        if (isdigit((unsigned char)*p))
            d = *p - '0';
        else if (isalpha((unsigned char)*p))
            d = (*p | 32) - 'a' + 10;
        else
            break;
        if (d >= base)
            break;
        if (v > (max - d) / base)
            *over = 1;
        else
            v = v * base + d;
    }
    if (end)
        *end = (char *)(p == start ? s : p);
    return v;
}

long long strtoll(const char *__restrict s, char **__restrict end, int base) {
    int neg, over;
    unsigned long long v = scan(s, end, base, (unsigned long long)LLONG_MAX + 1, &neg, &over);
    if (over || (!neg && v > (unsigned long long)LLONG_MAX)) {
        errno = ERANGE;
        return neg ? LLONG_MIN : LLONG_MAX;
    }
    return neg ? (long long)(0 - v) : (long long)v;
}

long strtol(const char *__restrict s, char **__restrict end, int base) {
    return strtoll(s, end, base);
}

unsigned long long strtoull(const char *__restrict s, char **__restrict end, int base) {
    int neg, over;
    unsigned long long v = scan(s, end, base, ULLONG_MAX, &neg, &over);
    if (over) {
        errno = ERANGE;
        return ULLONG_MAX;
    }
    return neg ? 0 - v : v;
}

unsigned long strtoul(const char *__restrict s, char **__restrict end, int base) {
    return strtoull(s, end, base);
}

intmax_t strtoimax(const char *__restrict s, char **__restrict end, int base) {
    return strtoll(s, end, base);
}

uintmax_t strtoumax(const char *__restrict s, char **__restrict end, int base) {
    return strtoull(s, end, base);
}

int atoi(const char *s) { return (int)strtol(s, 0, 10); }
long atol(const char *s) { return strtol(s, 0, 10); }
long long atoll(const char *s) { return strtoll(s, 0, 10); }
double atof(const char *s) { return strtod(s, 0); }

int abs(int a) { return a < 0 ? -a : a; }
long labs(long a) { return a < 0 ? -a : a; }
long long llabs(long long a) { return a < 0 ? -a : a; }
intmax_t imaxabs(intmax_t a) { return a < 0 ? -a : a; }
div_t div(int a, int b) { return (div_t){a / b, a % b}; }
ldiv_t ldiv(long a, long b) { return (ldiv_t){a / b, a % b}; }
lldiv_t lldiv(long long a, long long b) { return (lldiv_t){a / b, a % b}; }
imaxdiv_t imaxdiv(intmax_t a, intmax_t b) { return (imaxdiv_t){a / b, a % b}; }

static char *utoa(unsigned long v, char *buf, int base, int neg) {
    char tmp[72];
    int n = 0;
    if (base < 2 || base > 36) {
        buf[0] = 0;
        return buf;
    }
    do {
        int d = (int)(v % base);
        tmp[n++] = (char)(d < 10 ? '0' + d : 'a' + d - 10);
        v /= base;
    } while (v);
    char *p = buf;
    if (neg)
        *p++ = '-';
    while (n)
        *p++ = tmp[--n];
    *p = 0;
    return buf;
}

char *ltoa(long v, char *buf, int base) {
    if (base == 10 && v < 0)
        return utoa(0UL - (unsigned long)v, buf, 10, 1);
    return utoa((unsigned long)v, buf, base, 0);
}

char *itoa(int v, char *buf, int base) {
    if (base == 10 && v < 0)
        return utoa(0UL - (unsigned long)(long)v, buf, 10, 1);
    return utoa((unsigned)v, buf, base, 0);
}

char *ultoa(unsigned long v, char *buf, int base) {
    return utoa(v, buf, base, 0);
}

/* qsort: introsort-free quicksort (median of three, insertion sort for
 * small parts, the smaller part first so the stack stays short). */
static void swap(char *a, char *b, size_t n) {
    while (n--) {
        char t = *a;
        *a++ = *b;
        *b++ = t;
    }
}

void qsort(void *base, size_t n, size_t sz, int (*cmp)(const void *, const void *)) {
    char *b = base;
    while (n > 8) {
        char *lo = b, *hi = b + (n - 1) * sz, *mid = b + (n / 2) * sz;
        if (cmp(mid, lo) < 0)
            swap(mid, lo, sz);
        if (cmp(hi, mid) < 0) {
            swap(hi, mid, sz);
            if (cmp(mid, lo) < 0)
                swap(mid, lo, sz);
        }
        swap(mid, lo, sz);    /* the pivot at lo */
        char *i = lo, *j = hi + sz;
        for (;;) {
            do i += sz; while (i <= hi && cmp(i, lo) < 0);
            do j -= sz; while (cmp(j, lo) > 0);
            if (i >= j)
                break;
            swap(i, j, sz);
        }
        swap(lo, j, sz);
        size_t left = (j - b) / sz, right = n - left - 1;
        if (left < right) {
            qsort(b, left, sz, cmp);
            b = j + sz;
            n = right;
        } else {
            qsort(j + sz, right, sz, cmp);
            n = left;
        }
    }
    for (size_t i = 1; i < n; i++)
        for (char *p = b + i * sz; p > b && cmp(p - sz, p) > 0; p -= sz)
            swap(p - sz, p, sz);
}

void *bsearch(const void *key, const void *base, size_t n, size_t sz, int (*cmp)(const void *, const void *)) {
    const char *b = base;
    while (n) {
        const char *m = b + (n / 2) * sz;
        int c = cmp(key, m);
        if (c == 0)
            return (void *)m;
        if (c > 0) {
            b = m + sz;
            n -= n / 2 + 1;
        } else {
            n /= 2;
        }
    }
    return 0;
}

static unsigned long long seed = 1;

int rand_r(unsigned *s) {
    *s = *s * 1103515245u + 12345u;
    return (int)((*s >> 1) & RAND_MAX);
}

int rand(void) {
    seed = seed * 6364136223846793005ULL + 1442695040888963407ULL;
    return (int)(seed >> 33);
}

void srand(unsigned s) {
    seed = s - 1;
}

/* Multibyte characters: the "C" locale's single bytes. */
int mblen(const char *s, size_t n) {
    return !s ? 0 : n == 0 ? -1 : *s != 0;
}

int mbtowc(wchar_t *__restrict w, const char *__restrict s, size_t n) {
    if (!s)
        return 0;
    if (n == 0)
        return -1;
    if (w)
        *w = (unsigned char)*s;
    return *s != 0;
}

int wctomb(char *s, wchar_t w) {
    if (!s)
        return 0;
    if ((unsigned)w > 255) {
        errno = EILSEQ;
        return -1;
    }
    *s = (char)w;
    return 1;
}

size_t mbstowcs(wchar_t *__restrict d, const char *__restrict s, size_t n) {
    size_t i = 0;
    for (; (!d || i < n) && s[i]; i++)
        if (d)
            d[i] = (unsigned char)s[i];
    if (d && i < n)
        d[i] = 0;
    return i;
}

size_t wcstombs(char *__restrict d, const wchar_t *__restrict s, size_t n) {
    size_t i = 0;
    for (; (!d || i < n) && s[i]; i++) {
        if ((unsigned)s[i] > 255) {
            errno = EILSEQ;
            return (size_t)-1;
        }
        if (d)
            d[i] = (char)s[i];
    }
    if (d && i < n)
        d[i] = 0;
    return i;
}

char *setlocale(int cat, const char *name) {
    (void)cat;
    if (!name || !*name || !strcmp(name, "C") || !strcmp(name, "POSIX"))
        return "C";
    return 0;
}

struct lconv *localeconv(void) {
    static struct lconv c = {".", "", "", "", "", "", "", "", "", "", 127, 127, 127, 127, 127, 127, 127, 127, 127, 127, 127, 127, 127, 127};
    return &c;
}

char *optarg;
int optind = 1, opterr = 1, optopt;

int getopt(int argc, char *const argv[], const char *opts) {
    static int pos = 1;
    if (optind >= argc || !argv[optind] || argv[optind][0] != '-' || !argv[optind][1])
        return -1;
    if (!strcmp(argv[optind], "--")) {
        optind++;
        return -1;
    }
    int c = argv[optind][pos];
    const char *o = c == ':' ? 0 : strchr(opts, c);
    if (!o) {
        optopt = c;
        if (opterr && *opts != ':')
            fprintf(stderr, "%s: unknown option -%c\n", argv[0], c);
        if (!argv[optind][++pos])
            optind++, pos = 1;
        return '?';
    }
    if (o[1] == ':') {
        if (argv[optind][pos + 1]) {
            optarg = &argv[optind][pos + 1];
        } else if (optind + 1 < argc) {
            optarg = argv[++optind];
        } else {
            optopt = c;
            optind++;
            pos = 1;
            if (*opts == ':')
                return ':';
            if (opterr)
                fprintf(stderr, "%s: option -%c needs an argument\n", argv[0], c);
            return '?';
        }
        optind++;
        pos = 1;
        return c;
    }
    if (!argv[optind][++pos])
        optind++, pos = 1;
    return c;
}
