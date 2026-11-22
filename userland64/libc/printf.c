/* The printf family. Floating point is converted exactly (the decimal
 * expansion of a double is finite), then rounded half to even. */
#include "libc.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <stdint.h>
#include <unistd.h>

size_t _rs2_fwrite_unlocked(const void *p, size_t n, FILE *f);

/* ---- exact decimal digits of a double ---- */

#define LIMBS 120          /* base 1e9; 5^1074 * 2^53 needs ~86 */
#define DIGITS 1100

typedef struct {
    char d[DIGITS];        /* significant digits, no leading zeros */
    int len;               /* 0 for zero */
    int exp;               /* value = 0.d * 10^exp */
} decimal;

static void big_mul(uint32_t *b, int *n, uint32_t m) {
    uint64_t carry = 0;
    for (int i = 0; i < *n; i++) {
        uint64_t v = (uint64_t)b[i] * m + carry;
        b[i] = (uint32_t)(v % 1000000000u);
        carry = v / 1000000000u;
    }
    while (carry) {
        b[(*n)++] = (uint32_t)(carry % 1000000000u);
        carry /= 1000000000u;
    }
}

static void to_decimal(double x, decimal *out) {
    union { double d; uint64_t u; } v = {x};
    uint64_t m = v.u & ((1ULL << 52) - 1);
    int e = (int)(v.u >> 52 & 0x7ff);
    if (e == 0)
        e = -1074;
    else
        m |= 1ULL << 52, e -= 1075;
    out->len = 0;
    out->exp = 0;
    if (m == 0)
        return;
    while (!(m & 1))
        m >>= 1, e++;
    uint32_t b[LIMBS];
    int n = 0;
    while (m) {
        b[n++] = (uint32_t)(m % 1000000000u);
        m /= 1000000000u;
    }
    int k = 0;   /* value = big * 10^-k */
    if (e > 0) {
        for (; e >= 29; e -= 29)
            big_mul(b, &n, 1u << 29);
        if (e)
            big_mul(b, &n, 1u << e);
    } else if (e < 0) {
        k = -e;
        int r = k;
        for (; r >= 13; r -= 13)
            big_mul(b, &n, 1220703125u);   /* 5^13 */
        uint32_t p = 1;
        while (r--)
            p *= 5;
        if (p > 1)
            big_mul(b, &n, p);
    }
    /* The digits, most significant first. */
    char *d = out->d;
    int len = 0;
    char tmp[10];
    for (int i = n - 1; i >= 0; i--) {
        uint32_t limb = b[i];
        for (int j = 8; j >= 0; j--) {
            tmp[j] = (char)('0' + limb % 10);
            limb /= 10;
        }
        int j = 0;
        if (i == n - 1)
            while (j < 8 && tmp[j] == '0')
                j++;
        for (; j < 9; j++)
            d[len++] = tmp[j];
    }
    out->exp = len - k;
    while (len > 0 && d[len - 1] == '0')
        len--;
    out->len = len;
}

/* Keeps `keep` significant digits, rounding half to even. */
static void round_to(decimal *x, int keep) {
    if (keep >= x->len)
        return;
    if (keep < 0) {
        x->len = 0;
        return;
    }
    int up;
    char c = x->d[keep];
    if (c > '5')
        up = 1;
    else if (c < '5')
        up = 0;
    else {
        up = 0;
        for (int i = keep + 1; i < x->len; i++)
            if (x->d[i] != '0') {
                up = 1;
                break;
            }
        if (!up)
            up = keep > 0 && ((x->d[keep - 1] - '0') & 1);
    }
    x->len = keep;
    if (up) {
        int i = keep - 1;
        while (i >= 0 && x->d[i] == '9')
            i--;
        if (i < 0) {
            x->d[0] = '1';
            x->len = 1;
            x->exp++;
        } else {
            x->d[i]++;
            x->len = i + 1;
        }
    }
    while (x->len > 0 && x->d[x->len - 1] == '0')
        x->len--;
    if (x->len == 0)
        x->exp = 0;
}

static char dig(const decimal *x, int i) {
    return i >= 0 && i < x->len ? x->d[i] : '0';
}

/* ---- output ---- */

typedef struct {
    _rs2_out_fn out;
    void *ctx;
    long count;
} sink;

static void put(sink *s, const char *p, size_t n) {
    if (n) {
        s->out(s->ctx, p, n);
        s->count += n;
    }
}

static void pad(sink *s, char c, long n) {
    char b[64];
    if (n <= 0)
        return;
    memset(b, c, sizeof b);
    while (n > 0) {
        size_t k = n > 64 ? 64 : (size_t)n;
        put(s, b, k);
        n -= k;
    }
}

enum { MINUS = 1, PLUS = 2, SPACE = 4, HASH = 8, ZERO = 16 };

/* Writes prefix + body with the field width: body_len counts digits
 * that `emit` produces. */
static void field(sink *s, int flags, long width, const char *prefix, long body_len, int zero_ok,
                  void (*emit)(sink *, void *), void *arg) {
    long plen = (long)strlen(prefix), total = plen + body_len;
    if (!(flags & MINUS) && !((flags & ZERO) && zero_ok))
        pad(s, ' ', width - total);
    put(s, prefix, plen);
    if (!(flags & MINUS) && (flags & ZERO) && zero_ok)
        pad(s, '0', width - total);
    emit(s, arg);
    if (flags & MINUS)
        pad(s, ' ', width - total);
}

typedef struct {
    decimal *x;
    int kind;          /* 'f' or 'e' */
    int prec;
    int point;         /* print the decimal point */
    char e;            /* 'e' or 'E' */
    int ex;            /* the exponent for %e */
} fpbody;

static void emit_fp(sink *s, void *a) {
    fpbody *b = a;
    decimal *x = b->x;
    char buf[64];
    int n = 0;
#define OUT(c) do { buf[n++] = (c); if (n == 64) { put(s, buf, 64); n = 0; } } while (0)
    if (b->kind == 'f') {
        if (x->exp <= 0)
            OUT('0');
        else
            for (int i = 0; i < x->exp; i++)
                OUT(dig(x, i));
        if (b->point)
            OUT('.');
        for (int i = 0; i < b->prec; i++)
            OUT(dig(x, x->exp + i));
    } else {
        OUT(dig(x, 0));
        if (b->point)
            OUT('.');
        for (int i = 0; i < b->prec; i++)
            OUT(dig(x, 1 + i));
        OUT(b->e);
        int ex = b->ex;
        OUT(ex < 0 ? '-' : '+');
        if (ex < 0)
            ex = -ex;
        char t[8];
        int k = 0;
        do t[k++] = (char)('0' + ex % 10), ex /= 10; while (ex);
        if (k < 2)
            t[k++] = '0';
        while (k)
            OUT(t[--k]);
    }
    put(s, buf, n);
#undef OUT
}

static void emit_str(sink *s, void *a) {
    const char **p = a;
    put(s, p[0], (size_t)(p[1] - p[0]));
}

static void fmt_hex_float(sink *s, double v, int flags, long width, int prec, int upper, const char *sign) {
    union { double d; uint64_t u; } u = {v};
    uint64_t m = u.u & ((1ULL << 52) - 1);
    int e = (int)(u.u >> 52 & 0x7ff);
    int lead = 1;
    if (e == 0) {
        if (m == 0)
            lead = 0, e = 0;
        else
            lead = 0, e = -1022;
    } else {
        e -= 1023;
    }
    if (prec >= 0 && prec < 13) {
        int drop = (13 - prec) * 4;
        uint64_t full = (uint64_t)lead << 52 | m;
        uint64_t half = 1ULL << (drop - 1), rem = full & ((1ULL << drop) - 1);
        full >>= drop;
        if (rem > half || (rem == half && (full & 1)))
            full++;
        full <<= drop;
        lead = (int)(full >> 52);
        m = full & ((1ULL << 52) - 1);
        if (lead > 1) {   /* carried into 2.0: renormalise */
            lead = 1;
            m >>= 1;
            e++;
        }
    }
    const char *hx = upper ? "0123456789ABCDEF" : "0123456789abcdef";
    char body[64];
    int n = 0;
    body[n++] = (char)('0' + lead);
    int digits = prec >= 0 ? prec : 13;
    if (prec < 0)
        while (digits > 0 && !((m >> ((13 - digits) * 4)) & 15))
            digits--;
    if (digits || (flags & HASH))
        body[n++] = '.';
    for (int i = 0; i < digits; i++)
        body[n++] = i < 13 ? hx[(m >> ((12 - i) * 4)) & 15] : '0';
    n += snprintf(body + n, sizeof body - n, "%c%+d", upper ? 'P' : 'p', e);
    char prefix[8];
    snprintf(prefix, sizeof prefix, "%s0%c", sign, upper ? 'X' : 'x');
    const char *range[2] = {body, body + n};
    field(s, flags, width, prefix, n, 1, emit_str, range);
}

static void fmt_float(sink *s, double v, int conv, int flags, long width, int prec) {
    int upper = conv >= 'A' && conv <= 'Z';
    union { double d; uint64_t u; } u = {v};
    const char *sign = u.u >> 63 ? "-" : flags & PLUS ? "+" : flags & SPACE ? " " : "";
    if (u.u >> 63)
        v = -v;
    if (v != v || v == __builtin_inf()) {
        const char *t = v != v ? (upper ? "NAN" : "nan") : (upper ? "INF" : "inf");
        const char *range[2] = {t, t + 3};
        field(s, flags, width, sign, 3, 0, emit_str, range);
        return;
    }
    int lc = conv | 32;
    if (lc == 'a') {
        fmt_hex_float(s, v, flags, width, prec, upper, sign);
        return;
    }
    if (prec < 0)
        prec = 6;
    decimal x;
    to_decimal(v, &x);
    fpbody b = {&x, 'f', prec, prec > 0 || (flags & HASH), upper ? 'E' : 'e', 0};
    if (lc == 'g') {
        int p = prec ? prec : 1;
        decimal t = x;
        round_to(&t, p);
        int X = t.len ? t.exp - 1 : 0;
        if (p > X && X >= -4) {
            b.kind = 'f';
            b.prec = p - 1 - X;
            round_to(&x, x.exp + b.prec);
        } else {
            b.kind = 'e';
            b.prec = p - 1;
            x = t;
        }
        if (!(flags & HASH)) {
            /* Drop trailing zeros. */
            int sig = b.kind == 'f' ? (x.len - x.exp) : (x.len - 1);
            if (sig < 0)
                sig = 0;
            if (b.prec > sig)
                b.prec = sig;
        }
        b.point = b.prec > 0 || (flags & HASH);
    } else if (lc == 'e') {
        b.kind = 'e';
        round_to(&x, prec + 1);
    } else {
        round_to(&x, x.exp + prec);
    }
    long len;
    if (b.kind == 'e') {
        b.ex = x.len ? x.exp - 1 : 0;
        int ea = b.ex < 0 ? -b.ex : b.ex, ed = ea >= 100 ? (ea >= 1000 ? 4 : 3) : 2;
        len = 1 + b.point + b.prec + 2 + ed;
    } else {
        len = (x.exp > 0 ? x.exp : 1) + b.point + b.prec;
    }
    field(s, flags, width, sign, len, 1, emit_fp, &b);
}

typedef struct {
    const char *digits;
    int n;
    long zeros;     /* precision padding */
} intbody;

static void emit_int(sink *s, void *a) {
    intbody *b = a;
    pad(s, '0', b->zeros);
    put(s, b->digits, b->n);
}

static void fmt_int(sink *s, uintmax_t v, int neg, int base, int upper, int flags, long width, int prec, int conv) {
    char tmp[72];
    int n = 0;
    const char *hx = upper ? "0123456789ABCDEF" : "0123456789abcdef";
    while (v) {
        tmp[sizeof tmp - 1 - n++] = hx[v % base];
        v /= base;
    }
    if (prec < 0 && n == 0)
        tmp[sizeof tmp - 1 - n++] = '0';
    char prefix[4] = "";
    if (conv == 'd' || conv == 'i') {
        if (neg)
            strcpy(prefix, "-");
        else if (flags & PLUS)
            strcpy(prefix, "+");
        else if (flags & SPACE)
            strcpy(prefix, " ");
    } else if ((flags & HASH) && base == 16 && n && !(n == 1 && tmp[sizeof tmp - 1] == '0')) {
        strcpy(prefix, upper ? "0X" : "0x");
    } else if (conv == 'p') {
        strcpy(prefix, "0x");
    }
    long zeros = prec > n ? prec - n : 0;
    if ((flags & HASH) && base == 8 && zeros == 0 && (n == 0 || tmp[sizeof tmp - n] != '0'))
        zeros = 1;
    intbody b = {tmp + sizeof tmp - n, n, zeros};
    field(s, flags, width, prefix, n + zeros, prec < 0, emit_int, &b);
}

enum { L_NONE, L_HH, L_H, L_L, L_LL, L_J, L_Z, L_T, L_LD };

int _rs2_vformat(_rs2_out_fn out, void *ctx, const char *fmt, va_list ap) {
    sink s = {out, ctx, 0};
    va_list a;
    va_copy(a, ap);
    while (*fmt) {
        if (*fmt != '%') {
            const char *p = fmt;
            while (*p && *p != '%')
                p++;
            put(&s, fmt, p - fmt);
            fmt = p;
            continue;
        }
        fmt++;
        int flags = 0;
        for (;; fmt++) {
            if (*fmt == '-') flags |= MINUS;
            else if (*fmt == '+') flags |= PLUS;
            else if (*fmt == ' ') flags |= SPACE;
            else if (*fmt == '#') flags |= HASH;
            else if (*fmt == '0') flags |= ZERO;
            else if (*fmt == '\'') ;
            else break;
        }
        long width = 0;
        if (*fmt == '*') {
            width = va_arg(a, int);
            if (width < 0)
                flags |= MINUS, width = -width;
            fmt++;
        } else {
            while (*fmt >= '0' && *fmt <= '9')
                width = width * 10 + (*fmt++ - '0');
        }
        int prec = -1;
        if (*fmt == '.') {
            fmt++;
            if (*fmt == '*') {
                prec = va_arg(a, int);
                if (prec < 0)
                    prec = -1;
                fmt++;
            } else {
                prec = 0;
                while (*fmt >= '0' && *fmt <= '9')
                    prec = prec * 10 + (*fmt++ - '0');
            }
        }
        int len = L_NONE;
        switch (*fmt) {
        case 'h': len = fmt[1] == 'h' ? (fmt++, L_HH) : L_H; fmt++; break;
        case 'l': len = fmt[1] == 'l' ? (fmt++, L_LL) : L_L; fmt++; break;
        case 'q': len = L_LL; fmt++; break;
        case 'j': len = L_J; fmt++; break;
        case 'z': len = L_Z; fmt++; break;
        case 't': len = L_T; fmt++; break;
        case 'L': len = L_LD; fmt++; break;
        case 'I':
            if (fmt[1] == '6' && fmt[2] == '4')
                len = L_LL, fmt += 3;
            else if (fmt[1] == '3' && fmt[2] == '2')
                len = L_NONE, fmt += 3;
            break;
        }
        int conv = *fmt;
        if (!conv)
            break;
        fmt++;
        switch (conv) {
        case 'd': case 'i': {
            intmax_t v;
            switch (len) {
            case L_HH: v = (signed char)va_arg(a, int); break;
            case L_H: v = (short)va_arg(a, int); break;
            case L_L: case L_LL: case L_J: case L_Z: case L_T: case L_LD: v = va_arg(a, long); break;
            default: v = va_arg(a, int); break;
            }
            uintmax_t m = v < 0 ? 0 - (uintmax_t)v : (uintmax_t)v;
            fmt_int(&s, m, v < 0, 10, 0, flags, width, prec, conv);
            break;
        }
        case 'u': case 'o': case 'x': case 'X': case 'b': case 'B': {
            uintmax_t v;
            switch (len) {
            case L_HH: v = (unsigned char)va_arg(a, unsigned); break;
            case L_H: v = (unsigned short)va_arg(a, unsigned); break;
            case L_L: case L_LL: case L_J: case L_Z: case L_T: case L_LD: v = va_arg(a, unsigned long); break;
            default: v = va_arg(a, unsigned); break;
            }
            int base = conv == 'u' ? 10 : conv == 'o' ? 8 : (conv | 32) == 'b' ? 2 : 16;
            fmt_int(&s, v, 0, base, conv == 'X', flags, width, prec, conv);
            break;
        }
        case 'p': {
            void *p = va_arg(a, void *);
            fmt_int(&s, (uintptr_t)p, 0, 16, 0, flags & ~HASH, width, prec, 'p');
            break;
        }
        case 'c': {
            char c = (char)va_arg(a, int);
            const char *range[2] = {&c, &c + 1};
            field(&s, flags, width, "", 1, 0, emit_str, range);
            break;
        }
        case 's': {
            if (len == L_L) {
                const wchar_t *w = va_arg(a, const wchar_t *);
                if (!w)
                    w = L"(null)";
                size_t n = 0;
                while (w[n] && (prec < 0 || (long)n < prec))
                    n++;
                char *t = malloc(n + 1);
                if (t) {
                    for (size_t i = 0; i < n; i++)
                        t[i] = (char)w[i];
                    const char *range[2] = {t, t + n};
                    field(&s, flags, width, "", n, 0, emit_str, range);
                    free(t);
                }
                break;
            }
            const char *str = va_arg(a, const char *);
            if (!str)
                str = "(null)";
            size_t n = prec < 0 ? strlen(str) : strnlen(str, prec);
            const char *range[2] = {str, str + n};
            field(&s, flags, width, "", n, 0, emit_str, range);
            break;
        }
        case 'f': case 'F': case 'e': case 'E': case 'g': case 'G': case 'a': case 'A': {
            double v = len == L_LD ? (double)va_arg(a, long double) : va_arg(a, double);
            fmt_float(&s, v, conv, flags, width, prec);
            break;
        }
        case 'n': {
            void *p = va_arg(a, void *);
            switch (len) {
            case L_HH: *(signed char *)p = (signed char)s.count; break;
            case L_H: *(short *)p = (short)s.count; break;
            case L_L: case L_LL: case L_J: case L_Z: case L_T: *(long *)p = s.count; break;
            default: *(int *)p = (int)s.count; break;
            }
            break;
        }
        case 'm': {
            const char *m = strerror(errno);
            const char *range[2] = {m, m + strlen(m)};
            field(&s, flags, width, "", strlen(m), 0, emit_str, range);
            break;
        }
        case '%':
            put(&s, "%", 1);
            break;
        default:
            put(&s, fmt - 2, 2);
            break;
        }
    }
    va_end(a);
    return s.count > 0x7fffffff ? -1 : (int)s.count;
}

/* ---- the entry points ---- */

typedef struct {
    char *p;
    size_t room;   /* bytes that fit, the terminator not counted */
} strbuf;

static void to_str(void *c, const char *s, size_t n) {
    strbuf *b = c;
    size_t k = n < b->room ? n : b->room;
    memcpy(b->p, s, k);
    b->p += k;
    b->room -= k;
}

int vsnprintf(char *__restrict buf, size_t n, const char *__restrict fmt, va_list ap) {
    strbuf b = {buf, n ? n - 1 : 0};
    char dummy;
    if (!n)
        b.p = &dummy;
    int r = _rs2_vformat(to_str, &b, fmt, ap);
    if (n)
        *b.p = 0;
    return r;
}

int vsprintf(char *__restrict buf, const char *__restrict fmt, va_list ap) {
    return vsnprintf(buf, (size_t)INT32_MAX, fmt, ap);
}

int snprintf(char *__restrict buf, size_t n, const char *__restrict fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = vsnprintf(buf, n, fmt, ap);
    va_end(ap);
    return r;
}

int sprintf(char *__restrict buf, const char *__restrict fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = vsprintf(buf, fmt, ap);
    va_end(ap);
    return r;
}

static void to_file(void *c, const char *s, size_t n) {
    _rs2_fwrite_unlocked(s, n, c);
}

int vfprintf(FILE *__restrict f, const char *__restrict fmt, va_list ap) {
    flockfile(f);
    int r = _rs2_vformat(to_file, f, fmt, ap);
    if (ferror(f))
        r = -1;
    funlockfile(f);
    return r;
}

int vprintf(const char *__restrict fmt, va_list ap) {
    return vfprintf(stdout, fmt, ap);
}

int fprintf(FILE *__restrict f, const char *__restrict fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = vfprintf(f, fmt, ap);
    va_end(ap);
    return r;
}

int printf(const char *__restrict fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = vfprintf(stdout, fmt, ap);
    va_end(ap);
    return r;
}

typedef struct {
    char *p;
    size_t len, cap;
    int failed;
} dynbuf;

static void to_dyn(void *c, const char *s, size_t n) {
    dynbuf *b = c;
    if (b->failed)
        return;
    if (b->len + n + 1 > b->cap) {
        size_t cap = b->cap ? b->cap : 64;
        while (cap < b->len + n + 1)
            cap *= 2;
        char *np = realloc(b->p, cap);
        if (!np) {
            b->failed = 1;
            return;
        }
        b->p = np;
        b->cap = cap;
    }
    memcpy(b->p + b->len, s, n);
    b->len += n;
}

int vasprintf(char **out, const char *fmt, va_list ap) {
    dynbuf b = {0, 0, 0, 0};
    int r = _rs2_vformat(to_dyn, &b, fmt, ap);
    if (b.failed || (!b.p && !(b.p = malloc(1)))) {
        free(b.p);
        *out = 0;
        return -1;
    }
    b.p[b.len] = 0;
    *out = b.p;
    return r;
}

int asprintf(char **out, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = vasprintf(out, fmt, ap);
    va_end(ap);
    return r;
}

int vdprintf(int fd, const char *__restrict fmt, va_list ap) {
    char *s;
    int r = vasprintf(&s, fmt, ap);
    if (r < 0)
        return r;
    if (write(fd, s, (size_t)r) != r)
        r = -1;
    free(s);
    return r;
}

int dprintf(int fd, const char *__restrict fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = vdprintf(fd, fmt, ap);
    va_end(ap);
    return r;
}
