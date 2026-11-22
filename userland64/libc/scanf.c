/* The scanf family. */
#include "libc.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <stdarg.h>
#include <stdint.h>

enum { L_NONE, L_HH, L_H, L_L, L_LL, L_J, L_Z, L_T, L_LD };

typedef struct {
    _rs2_in_t *in;
    long count;   /* characters read */
} src;

static int get(src *s) {
    int c = s->in->get(s->in->ctx);
    if (c != EOF)
        s->count++;
    return c;
}

static void unget(src *s, int c) {
    if (c != EOF) {
        s->in->unget(c, s->in->ctx);
        s->count--;
    }
}

static void store_int(void *p, int len, uintmax_t v) {
    switch (len) {
    case L_HH: *(char *)p = (char)v; break;
    case L_H: *(short *)p = (short)v; break;
    case L_L: case L_LL: case L_J: case L_Z: case L_T: case L_LD: *(long *)p = (long)v; break;
    default: *(int *)p = (int)v; break;
    }
}

int _rs2_vscan(_rs2_in_t *in, const char *fmt, va_list ap) {
    src s = {in, 0};
    int assigned = 0, any_input = 0;
    va_list a;
    va_copy(a, ap);
    while (*fmt) {
        unsigned char f = (unsigned char)*fmt;
        if (isspace(f)) {
            int c;
            while ((c = get(&s)) != EOF && isspace(c))
                ;
            unget(&s, c);
            fmt++;
            continue;
        }
        if (f != '%' || fmt[1] == '%') {
            if (f == '%')
                fmt++;
            int c = get(&s);
            if (f == '%')
                while (c != EOF && isspace(c))
                    c = get(&s);
            if (c != (unsigned char)*fmt) {
                unget(&s, c);
                if (c == EOF && !any_input)
                    goto input_failure;
                goto done;
            }
            any_input = 1;
            fmt++;
            continue;
        }
        fmt++;
        int suppress = 0;
        if (*fmt == '*')
            suppress = 1, fmt++;
        long width = 0;
        while (isdigit((unsigned char)*fmt))
            width = width * 10 + (*fmt++ - '0');
        int len = L_NONE;
        switch (*fmt) {
        case 'h': len = fmt[1] == 'h' ? (fmt++, L_HH) : L_H; fmt++; break;
        case 'l': len = fmt[1] == 'l' ? (fmt++, L_LL) : L_L; fmt++; break;
        case 'q': len = L_LL; fmt++; break;
        case 'j': len = L_J; fmt++; break;
        case 'z': len = L_Z; fmt++; break;
        case 't': len = L_T; fmt++; break;
        case 'L': len = L_LD; fmt++; break;
        }
        int conv = *fmt++;
        if (!conv)
            break;
        if (conv == 'n') {
            if (!suppress)
                store_int(va_arg(a, void *), len, (uintmax_t)s.count);
            continue;
        }
        int c;
        if (conv != 'c' && conv != '[') {
            while ((c = get(&s)) != EOF && isspace(c))
                ;
            unget(&s, c);
        }
        switch (conv) {
        case 'c': {
            if (!width)
                width = 1;
            char *d = suppress ? 0 : va_arg(a, char *);
            long i = 0;
            for (; i < width && (c = get(&s)) != EOF; i++)
                if (d)
                    d[i] = (char)c;
            if (i == 0)
                goto input_failure_or_done;
            any_input = 1;
            if (d)
                assigned++;
            break;
        }
        case 's': case '[': {
            unsigned char set[256];
            if (conv == '[') {
                int neg = 0;
                if (*fmt == '^')
                    neg = 1, fmt++;
                memset(set, neg, sizeof set);
                if (*fmt == ']')
                    set[']'] = !neg, fmt++;
                while (*fmt && *fmt != ']') {
                    if (fmt[1] == '-' && fmt[2] && fmt[2] != ']') {
                        for (int ch = (unsigned char)fmt[0]; ch <= (unsigned char)fmt[2]; ch++)
                            set[ch] = !neg;
                        fmt += 3;
                    } else {
                        set[(unsigned char)*fmt++] = !neg;
                    }
                }
                if (*fmt == ']')
                    fmt++;
            } else {
                for (int ch = 0; ch < 256; ch++)
                    set[ch] = !isspace(ch);
            }
            char *d = suppress ? 0 : va_arg(a, char *);
            long i = 0;
            while ((!width || i < width) && (c = get(&s)) != EOF) {
                if (!set[c]) {
                    unget(&s, c);
                    break;
                }
                if (d)
                    d[i] = (char)c;
                i++;
            }
            if (i == 0)
                goto input_failure_or_done;
            any_input = 1;
            if (d) {
                d[i] = 0;
                assigned++;
            }
            break;
        }
        case 'd': case 'i': case 'u': case 'o': case 'x': case 'X': case 'p': {
            int base = conv == 'o' ? 8 : conv == 'x' || conv == 'X' || conv == 'p' ? 16 : conv == 'i' ? 0 : 10;
            char buf[80];
            int n = 0;
            long max = width ? width : 79;
            if (max > 79)
                max = 79;
            c = get(&s);
            if ((c == '+' || c == '-') && n < max) {
                buf[n++] = (char)c;
                c = get(&s);
            }
            if (c == '0' && n < max) {
                buf[n++] = '0';
                c = get(&s);
                if ((c | 32) == 'x' && (base == 0 || base == 16) && n < max) {
                    buf[n++] = (char)c;
                    c = get(&s);
                    base = 16;
                } else if (base == 0) {
                    base = 8;
                }
            }
            if (base == 0)
                base = 10;
            while (c != EOF && n < max) {
                int dv = isdigit(c) ? c - '0' : isalpha(c) ? (c | 32) - 'a' + 10 : 99;
                if (dv >= base)
                    break;
                buf[n++] = (char)c;
                c = get(&s);
            }
            unget(&s, c);
            buf[n] = 0;
            /* "0x" with no digits after it: just the 0 counts. */
            int ok = n > 0 && isxdigit((unsigned char)buf[n - 1]);
            if (!ok && n >= 2 && (buf[n - 1] | 32) == 'x') {
                ok = 1;
                buf[n - 1] = 0;
            }
            if (!ok)
                goto input_failure_or_done;
            any_input = 1;
            if (!suppress) {
                uintmax_t v = conv == 'd' || conv == 'i' ? (uintmax_t)strtoll(buf, 0, base) : strtoull(buf, 0, base);
                if (conv == 'p')
                    *va_arg(a, void **) = (void *)(uintptr_t)v;
                else
                    store_int(va_arg(a, void *), len, v);
                assigned++;
            }
            break;
        }
        case 'f': case 'F': case 'e': case 'E': case 'g': case 'G': case 'a': case 'A': {
            char buf[512];
            int n = 0;
            long max = width ? width : 511;
            if (max > 511)
                max = 511;
            /* Take the longest prefix that can begin a number. */
            int seen_digit = 0, seen_point = 0, seen_exp = 0, hex = 0;
            c = get(&s);
            if ((c == '+' || c == '-') && n < max)
                buf[n++] = (char)c, c = get(&s);
            if ((c | 32) == 'i' || (c | 32) == 'n') {
                const char *words[] = {"infinity", "nan"};
                const char *w = (c | 32) == 'i' ? words[0] : words[1];
                int k = 0;
                while (w[k] && (c | 32) == w[k] && n < max) {
                    buf[n++] = (char)c;
                    k++;
                    c = get(&s);
                }
                unget(&s, c);
                if (!(k == 3 || k == 8)) {
                    goto input_failure_or_done;
                }
                buf[n] = 0;
            } else {
                if (c == '0' && n < max) {
                    buf[n++] = '0';
                    seen_digit = 1;
                    c = get(&s);
                    if ((c | 32) == 'x' && n < max) {
                        buf[n++] = (char)c;
                        hex = 1;
                        seen_digit = 0;
                        c = get(&s);
                    }
                }
                while (c != EOF && n < max) {
                    if (hex ? isxdigit(c) : isdigit(c)) {
                        seen_digit = 1;
                    } else if (c == '.' && !seen_point && !seen_exp) {
                        seen_point = 1;
                    } else if (seen_digit && !seen_exp && (c | 32) == (hex ? 'p' : 'e')) {
                        seen_exp = 1;
                        buf[n++] = (char)c;
                        c = get(&s);
                        if ((c == '+' || c == '-') && n < max) {
                            buf[n++] = (char)c;
                            c = get(&s);
                        }
                        continue;
                    } else {
                        break;
                    }
                    buf[n++] = (char)c;
                    c = get(&s);
                }
                unget(&s, c);
                buf[n] = 0;
                if (!seen_digit)
                    goto input_failure_or_done;
            }
            any_input = 1;
            if (!suppress) {
                if (len == L_L)
                    *va_arg(a, double *) = strtod(buf, 0);
                else if (len == L_LD)
                    *va_arg(a, long double *) = strtold(buf, 0);
                else
                    *va_arg(a, float *) = strtof(buf, 0);
                assigned++;
            }
            break;
        }
        default:
            goto done;
        }
        continue;
    input_failure_or_done:
        if (c == EOF && !any_input)
            goto input_failure;
        goto done;
    }
done:
    va_end(a);
    return assigned;
input_failure:
    va_end(a);
    return assigned ? assigned : EOF;
}

static int str_get(void *c) {
    const unsigned char **p = c;
    return **p ? *(*p)++ : EOF;
}

static void str_unget(int ch, void *c) {
    (void)ch;
    const char **p = c;
    (*p)--;
}

int vsscanf(const char *__restrict str, const char *__restrict fmt, va_list ap) {
    const char *p = str;
    _rs2_in_t in = {str_get, str_unget, &p};
    return _rs2_vscan(&in, fmt, ap);
}

int sscanf(const char *__restrict str, const char *__restrict fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = vsscanf(str, fmt, ap);
    va_end(ap);
    return r;
}

static int file_get(void *c) {
    return getc_unlocked(c);
}

static void file_unget(int ch, void *c) {
    ungetc(ch, c);
}

int vfscanf(FILE *__restrict f, const char *__restrict fmt, va_list ap) {
    _rs2_in_t in = {file_get, file_unget, f};
    flockfile(f);
    int r = _rs2_vscan(&in, fmt, ap);
    funlockfile(f);
    return r;
}

int fscanf(FILE *__restrict f, const char *__restrict fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = vfscanf(f, fmt, ap);
    va_end(ap);
    return r;
}

int vscanf(const char *__restrict fmt, va_list ap) {
    return vfscanf(stdin, fmt, ap);
}

int scanf(const char *__restrict fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int r = vfscanf(stdin, fmt, ap);
    va_end(ap);
    return r;
}
