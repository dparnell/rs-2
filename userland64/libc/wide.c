/* <wchar.h>, <wctype.h>, <uchar.h>. Multibyte text is single bytes. */
#include "libc.h"
#include <wchar.h>
#include <wctype.h>
#include <uchar.h>
#include <ctype.h>
#include <inttypes.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>

/* ---- conversion ---- */

wint_t btowc(int c) { return c == EOF ? WEOF : (unsigned char)c; }
int wctob(wint_t c) { return c < 256 ? (int)c : EOF; }
int mbsinit(const mbstate_t *s) { return !s || !s->__state[0]; }
size_t mbrtowc(wchar_t *__restrict w, const char *__restrict s, size_t n, mbstate_t *__restrict st) {
    (void)st;
    if (!s)
        return 0;
    if (!n)
        return (size_t)-2;
    if (w)
        *w = (unsigned char)*s;
    return *s != 0;
}
size_t mbrlen(const char *__restrict s, size_t n, mbstate_t *__restrict st) { return mbrtowc(0, s, n, st); }
size_t wcrtomb(char *__restrict s, wchar_t w, mbstate_t *__restrict st) {
    (void)st;
    if (!s)
        return 1;
    if ((unsigned)w > 255) {
        errno = EILSEQ;
        return (size_t)-1;
    }
    *s = (char)w;
    return 1;
}
size_t mbsnrtowcs(wchar_t *__restrict d, const char **__restrict sp, size_t nms, size_t len, mbstate_t *__restrict st) {
    (void)st;
    const char *s = *sp;
    size_t i = 0;
    for (; i < nms && (!d || i < len); i++) {
        if (d)
            d[i] = (unsigned char)s[i];
        if (!s[i]) {
            if (d)
                *sp = 0;
            return i;
        }
    }
    if (d)
        *sp = s + i;
    return i;
}
size_t mbsrtowcs(wchar_t *__restrict d, const char **__restrict sp, size_t len, mbstate_t *__restrict st) {
    return mbsnrtowcs(d, sp, (size_t)-1, len, st);
}
size_t wcsnrtombs(char *__restrict d, const wchar_t **__restrict sp, size_t nwc, size_t len, mbstate_t *__restrict st) {
    (void)st;
    const wchar_t *s = *sp;
    size_t i = 0;
    for (; i < nwc && (!d || i < len); i++) {
        if ((unsigned)s[i] > 255) {
            if (d)
                *sp = s + i;
            errno = EILSEQ;
            return (size_t)-1;
        }
        if (d)
            d[i] = (char)s[i];
        if (!s[i]) {
            if (d)
                *sp = 0;
            return i;
        }
    }
    if (d)
        *sp = s + i;
    return i;
}
size_t wcsrtombs(char *__restrict d, const wchar_t **__restrict sp, size_t len, mbstate_t *__restrict st) {
    return wcsnrtombs(d, sp, (size_t)-1, len, st);
}
size_t mbrtoc16(char16_t *__restrict c, const char *__restrict s, size_t n, mbstate_t *__restrict st) {
    wchar_t w;
    size_t r = mbrtowc(&w, s, n, st);
    if (s && c && r <= 1)
        *c = (char16_t)w;
    return r;
}
size_t c16rtomb(char *__restrict s, char16_t c, mbstate_t *__restrict st) { return wcrtomb(s, c, st); }
size_t mbrtoc32(char32_t *__restrict c, const char *__restrict s, size_t n, mbstate_t *__restrict st) {
    wchar_t w;
    size_t r = mbrtowc(&w, s, n, st);
    if (s && c && r <= 1)
        *c = (char32_t)w;
    return r;
}
size_t c32rtomb(char *__restrict s, char32_t c, mbstate_t *__restrict st) { return wcrtomb(s, (wchar_t)c, st); }

/* ---- strings ---- */

size_t wcslen(const wchar_t *s) {
    size_t n = 0;
    while (s[n])
        n++;
    return n;
}
size_t wcsnlen(const wchar_t *s, size_t m) {
    size_t n = 0;
    while (n < m && s[n])
        n++;
    return n;
}
wchar_t *wmemcpy(wchar_t *__restrict d, const wchar_t *__restrict s, size_t n) { return memcpy(d, s, n * sizeof *d); }
wchar_t *wmemmove(wchar_t *d, const wchar_t *s, size_t n) { return memmove(d, s, n * sizeof *d); }
wchar_t *wmemset(wchar_t *d, wchar_t c, size_t n) {
    for (size_t i = 0; i < n; i++)
        d[i] = c;
    return d;
}
int wmemcmp(const wchar_t *a, const wchar_t *b, size_t n) {
    for (; n; n--, a++, b++)
        if (*a != *b)
            return *a < *b ? -1 : 1;
    return 0;
}
wchar_t *wmemchr(const wchar_t *s, wchar_t c, size_t n) {
    for (; n; n--, s++)
        if (*s == c)
            return (wchar_t *)s;
    return 0;
}
wchar_t *wcscpy(wchar_t *__restrict d, const wchar_t *__restrict s) { return wmemcpy(d, s, wcslen(s) + 1); }
wchar_t *wcsncpy(wchar_t *__restrict d, const wchar_t *__restrict s, size_t n) {
    size_t k = wcsnlen(s, n);
    wmemcpy(d, s, k);
    wmemset(d + k, 0, n - k);
    return d;
}
wchar_t *wcscat(wchar_t *__restrict d, const wchar_t *__restrict s) {
    wcscpy(d + wcslen(d), s);
    return d;
}
wchar_t *wcsncat(wchar_t *__restrict d, const wchar_t *__restrict s, size_t n) {
    wchar_t *p = d + wcslen(d);
    size_t k = wcsnlen(s, n);
    wmemcpy(p, s, k);
    p[k] = 0;
    return d;
}
int wcsncmp(const wchar_t *a, const wchar_t *b, size_t n) {
    for (; n; n--, a++, b++) {
        if (*a != *b)
            return *a < *b ? -1 : 1;
        if (!*a)
            break;
    }
    return 0;
}
int wcscmp(const wchar_t *a, const wchar_t *b) { return wcsncmp(a, b, (size_t)-1); }
int wcscoll(const wchar_t *a, const wchar_t *b) { return wcscmp(a, b); }
size_t wcsxfrm(wchar_t *__restrict d, const wchar_t *__restrict s, size_t n) {
    size_t k = wcslen(s);
    if (k < n)
        wcscpy(d, s);
    return k;
}
wchar_t *wcschr(const wchar_t *s, wchar_t c) {
    for (;; s++) {
        if (*s == c)
            return (wchar_t *)s;
        if (!*s)
            return 0;
    }
}
wchar_t *wcsrchr(const wchar_t *s, wchar_t c) {
    for (const wchar_t *p = s + wcslen(s);; p--) {
        if (*p == c)
            return (wchar_t *)p;
        if (p == s)
            return 0;
    }
}
size_t wcsspn(const wchar_t *s, const wchar_t *set) {
    size_t n = 0;
    while (s[n] && wcschr(set, s[n]))
        n++;
    return n;
}
size_t wcscspn(const wchar_t *s, const wchar_t *set) {
    size_t n = 0;
    while (s[n] && !wcschr(set, s[n]))
        n++;
    return n;
}
wchar_t *wcspbrk(const wchar_t *s, const wchar_t *set) {
    s += wcscspn(s, set);
    return *s ? (wchar_t *)s : 0;
}
wchar_t *wcsstr(const wchar_t *h, const wchar_t *n) {
    size_t k = wcslen(n);
    if (!k)
        return (wchar_t *)h;
    for (; *h; h++)
        if (!wcsncmp(h, n, k))
            return (wchar_t *)h;
    return 0;
}
wchar_t *wcstok(wchar_t *__restrict s, const wchar_t *__restrict set, wchar_t **__restrict save) {
    if (!s && !(s = *save))
        return 0;
    s += wcsspn(s, set);
    if (!*s)
        return *save = 0;
    wchar_t *e = s + wcscspn(s, set);
    *save = *e ? e + 1 : 0;
    *e = 0;
    return s;
}
wchar_t *wcsdup(const wchar_t *s) {
    size_t n = wcslen(s) + 1;
    wchar_t *d = malloc(n * sizeof *d);
    return d ? wmemcpy(d, s, n) : 0;
}
void *memmem(const void *h, size_t hn, const void *n, size_t nn) {
    const char *p = h;
    for (; hn >= nn; hn--, p++)
        if (!memcmp(p, n, nn))
            return (void *)p;
    return 0;
}

/* ---- numbers: the narrow functions on a narrow copy ---- */

/* Copies s up to the first character no number has; the caller frees. */
static char *narrow(const wchar_t *s, char *buf, size_t size) {
    size_t n = 0;
    while (s[n] > 0 && s[n] < 128)
        n++;
    char *d = n < size ? buf : malloc(n + 1);
    if (!d)
        return 0;
    for (size_t i = 0; i < n; i++)
        d[i] = (char)s[i];
    d[n] = 0;
    return d;
}
#define NUMBER(type, name, call) \
    char buf[128], *e, *b = narrow(s, buf, sizeof buf); \
    if (!b) \
        return 0; \
    type r = call; \
    if (end) \
        *end = (wchar_t *)s + (e - b); \
    if (b != buf) \
        free(b); \
    return r;
long long wcstoll(const wchar_t *__restrict s, wchar_t **__restrict end, int base) { NUMBER(long long, wcstoll, strtoll(b, &e, base)) }
unsigned long long wcstoull(const wchar_t *__restrict s, wchar_t **__restrict end, int base) { NUMBER(unsigned long long, wcstoull, strtoull(b, &e, base)) }
long wcstol(const wchar_t *__restrict s, wchar_t **__restrict end, int base) { return wcstoll(s, end, base); }
unsigned long wcstoul(const wchar_t *__restrict s, wchar_t **__restrict end, int base) { return wcstoull(s, end, base); }
intmax_t wcstoimax(const wchar_t *__restrict s, wchar_t **__restrict end, int base) { return wcstoll(s, end, base); }
uintmax_t wcstoumax(const wchar_t *__restrict s, wchar_t **__restrict end, int base) { return wcstoull(s, end, base); }
double wcstod(const wchar_t *__restrict s, wchar_t **__restrict end) { NUMBER(double, wcstod, strtod(b, &e)) }
float wcstof(const wchar_t *__restrict s, wchar_t **__restrict end) { NUMBER(float, wcstof, strtof(b, &e)) }
long double wcstold(const wchar_t *__restrict s, wchar_t **__restrict end) { NUMBER(long double, wcstold, strtold(b, &e)) }

/* ---- character classes: ASCII ---- */

#define CLASS(n) int isw##n(wint_t c) { return c < 128 && is##n((int)c); }
CLASS(alnum) CLASS(alpha) CLASS(blank) CLASS(cntrl) CLASS(digit) CLASS(graph)
CLASS(lower) CLASS(print) CLASS(punct) CLASS(space) CLASS(upper) CLASS(xdigit)
wint_t towlower(wint_t c) { return c < 128 ? (wint_t)tolower((int)c) : c; }
wint_t towupper(wint_t c) { return c < 128 ? (wint_t)toupper((int)c) : c; }
static const struct { const char *name; int (*fn)(wint_t); } classes[] = {
    {"alnum", iswalnum}, {"alpha", iswalpha}, {"blank", iswblank}, {"cntrl", iswcntrl},
    {"digit", iswdigit}, {"graph", iswgraph}, {"lower", iswlower}, {"print", iswprint},
    {"punct", iswpunct}, {"space", iswspace}, {"upper", iswupper}, {"xdigit", iswxdigit}};
wctype_t wctype(const char *n) {
    for (unsigned i = 0; i < 12; i++)
        if (!strcmp(n, classes[i].name))
            return i + 1;
    return 0;
}
int iswctype(wint_t c, wctype_t t) { return t >= 1 && t <= 12 && classes[t - 1].fn(c); }
static const int lower_tag, upper_tag;
wctrans_t wctrans(const char *n) {
    return !strcmp(n, "tolower") ? &lower_tag : !strcmp(n, "toupper") ? &upper_tag : 0;
}
wint_t towctrans(wint_t c, wctrans_t t) { return t == &lower_tag ? towlower(c) : t == &upper_tag ? towupper(c) : c; }

/* ---- streams ---- */

int fwide(FILE *f, int mode) { (void)f; return mode; }
wint_t fgetwc(FILE *f) {
    int c = getc(f);
    return c == EOF ? WEOF : (wint_t)c;
}
wint_t getwc(FILE *f) { return fgetwc(f); }
wint_t getwchar(void) { return fgetwc(stdin); }
wint_t ungetwc(wint_t c, FILE *f) { return c > 255 || ungetc((int)c, f) == EOF ? WEOF : c; }
wint_t fputwc(wchar_t c, FILE *f) {
    if ((unsigned)c > 255) {
        errno = EILSEQ;
        return WEOF;
    }
    return putc(c, f) == EOF ? WEOF : (wint_t)c;
}
wint_t putwc(wchar_t c, FILE *f) { return fputwc(c, f); }
wint_t putwchar(wchar_t c) { return fputwc(c, stdout); }
wchar_t *fgetws(wchar_t *__restrict s, int n, FILE *__restrict f) {
    int i = 0;
    while (i < n - 1) {
        wint_t c = fgetwc(f);
        if (c == WEOF)
            break;
        s[i++] = (wchar_t)c;
        if (c == '\n')
            break;
    }
    if (i == 0)
        return 0;
    s[i] = 0;
    return s;
}
int fputws(const wchar_t *__restrict s, FILE *__restrict f) {
    for (; *s; s++)
        if (fputwc(*s, f) == WEOF)
            return -1;
    return 0;
}

/* Formatted wide I/O goes through the narrow functions: characters above
 * 255 in the format or in %ls arguments fail or are cut. */
static char *narrow_format(const wchar_t *f) {
    size_t n = wcslen(f);
    char *d = malloc(n + 1);
    if (!d)
        return 0;
    for (size_t i = 0; i <= n; i++) {
        if ((unsigned)f[i] > 255) {
            free(d);
            errno = EILSEQ;
            return 0;
        }
        d[i] = (char)f[i];
    }
    return d;
}
int vswprintf(wchar_t *__restrict s, size_t n, const wchar_t *__restrict fmt, va_list ap) {
    char *f = narrow_format(fmt), *out = 0;
    if (!f)
        return -1;
    int r = vasprintf(&out, f, ap);
    free(f);
    if (r < 0 || (size_t)r >= n) {
        free(out);
        return -1;
    }
    for (int i = 0; i <= r; i++)
        s[i] = (unsigned char)out[i];
    free(out);
    return r;
}
int vfwprintf(FILE *__restrict f, const wchar_t *__restrict fmt, va_list ap) {
    char *nf = narrow_format(fmt);
    if (!nf)
        return -1;
    int r = vfprintf(f, nf, ap);
    free(nf);
    return r;
}
int vwprintf(const wchar_t *__restrict fmt, va_list ap) { return vfwprintf(stdout, fmt, ap); }
int vfwscanf(FILE *__restrict f, const wchar_t *__restrict fmt, va_list ap) {
    char *nf = narrow_format(fmt);
    if (!nf)
        return -1;
    int r = vfscanf(f, nf, ap);
    free(nf);
    return r;
}
int vwscanf(const wchar_t *__restrict fmt, va_list ap) { return vfwscanf(stdin, fmt, ap); }
int vswscanf(const wchar_t *__restrict s, const wchar_t *__restrict fmt, va_list ap) {
    char *nf = narrow_format(fmt), *ns = nf ? narrow_format(s) : 0;
    int r = nf && ns ? vsscanf(ns, nf, ap) : -1;
    free(nf);
    free(ns);
    return r;
}
#define VARIADIC(call) va_list ap; va_start(ap, fmt); int r = call; va_end(ap); return r;
int swprintf(wchar_t *__restrict s, size_t n, const wchar_t *__restrict fmt, ...) { VARIADIC(vswprintf(s, n, fmt, ap)) }
int fwprintf(FILE *__restrict f, const wchar_t *__restrict fmt, ...) { VARIADIC(vfwprintf(f, fmt, ap)) }
int wprintf(const wchar_t *__restrict fmt, ...) { VARIADIC(vwprintf(fmt, ap)) }
int swscanf(const wchar_t *__restrict s, const wchar_t *__restrict fmt, ...) { VARIADIC(vswscanf(s, fmt, ap)) }
int fwscanf(FILE *__restrict f, const wchar_t *__restrict fmt, ...) { VARIADIC(vfwscanf(f, fmt, ap)) }
int wscanf(const wchar_t *__restrict fmt, ...) { VARIADIC(vwscanf(fmt, ap)) }
size_t wcsftime(wchar_t *__restrict s, size_t n, const wchar_t *__restrict fmt, const struct tm *__restrict tm) {
    char *f = narrow_format(fmt), *b = f ? malloc(n + 1) : 0;
    size_t r = b ? strftime(b, n, f, tm) : 0;
    for (size_t i = 0; r && i <= r; i++)
        s[i] = (unsigned char)b[i];
    free(f);
    free(b);
    return r;
}
