/* strtod and friends. Decimal input: up to 19 significant digits are
 * exact; the power of ten is applied in 80-bit long double, so results
 * are correctly rounded except, rarely, very near a halfway point.
 * Hexadecimal input is exact. */
#include "libc.h"
#include <stdlib.h>
#include <ctype.h>
#include <string.h>
#include <math.h>

static int match(const char *s, const char *word) {
    int i = 0;
    for (; word[i]; i++)
        if ((s[i] | 32) != word[i])
            return 0;
    return i;
}

static long double pow10l_(int e) {
    static const long double p[] = {1e1L, 1e2L, 1e4L, 1e8L, 1e16L, 1e32L, 1e64L, 1e128L, 1e256L, 1e512L, 1e1024L, 1e2048L, 1e4096L};
    long double r = 1.0L;
    for (int i = 0; e && i < 13; i++, e >>= 1)
        if (e & 1)
            r *= p[i];
    return e ? __builtin_infl() : r;
}

long double strtold(const char *__restrict s, char **__restrict end) {
    const char *p = s;
    while (isspace((unsigned char)*p))
        p++;
    int neg = 0;
    if (*p == '+' || *p == '-')
        neg = *p++ == '-';
    long double r;
    int k;
    if ((k = match(p, "infinity")) || (k = match(p, "inf"))) {
        if (end)
            *end = (char *)p + k;
        return neg ? -__builtin_infl() : __builtin_infl();
    }
    if ((k = match(p, "nan"))) {
        p += k;
        if (*p == '(') {
            const char *q = p + 1;
            while (isalnum((unsigned char)*q) || *q == '_')
                q++;
            if (*q == ')')
                p = q + 1;
        }
        if (end)
            *end = (char *)p;
        return neg ? -__builtin_nanl("") : __builtin_nanl("");
    }
    if (p[0] == '0' && (p[1] | 32) == 'x' && (isxdigit((unsigned char)p[2]) || (p[2] == '.' && isxdigit((unsigned char)p[3])))) {
        p += 2;
        unsigned long long m = 0;
        int e = 0, any = 0, sticky = 0, seen_point = 0;
        for (;; p++) {
            int d;
            if (*p == '.' && !seen_point) {
                seen_point = 1;
                continue;
            }
            if (isdigit((unsigned char)*p))
                d = *p - '0';
            else if (isxdigit((unsigned char)*p))
                d = (*p | 32) - 'a' + 10;
            else
                break;
            any = 1;
            if (m >> 60) {
                sticky |= d != 0;
                if (!seen_point)
                    e += 4;
            } else {
                m = m << 4 | d;
                if (seen_point)
                    e -= 4;
            }
        }
        (void)any;
        if ((*p | 32) == 'p') {
            const char *q = p + 1;
            int en = 0;
            if (*q == '+' || *q == '-')
                en = *q++ == '-';
            if (isdigit((unsigned char)*q)) {
                long x = 0;
                while (isdigit((unsigned char)*q)) {
                    if (x < 100000)
                        x = x * 10 + (*q - '0');
                    q++;
                }
                e += en ? -x : x;
                p = q;
            }
        }
        if (end)
            *end = (char *)p;
        m |= sticky;
        r = ldexpl((long double)m, e);
        if (r == __builtin_infl() || (r == 0 && m))
            errno = ERANGE;
        return neg ? -r : r;
    }
    unsigned long long m = 0;
    int digits = 0, e = 0, any = 0, sticky = 0;
    for (; isdigit((unsigned char)*p); p++) {
        any = 1;
        if (digits < 19) {
            m = m * 10 + (*p - '0');
            if (m)
                digits++;
        } else {
            e++;
            sticky |= *p != '0';
        }
    }
    if (*p == '.') {
        const char *q = p + 1;
        if (any || isdigit((unsigned char)*q)) {
            for (p = q; isdigit((unsigned char)*p); p++) {
                any = 1;
                if (digits < 19) {
                    m = m * 10 + (*p - '0');
                    if (m)
                        digits++;
                    e--;
                } else {
                    sticky |= *p != '0';
                }
            }
        }
    }
    if (!any) {
        if (end)
            *end = (char *)s;
        return 0;
    }
    if ((*p | 32) == 'e') {
        const char *q = p + 1;
        int en = 0;
        if (*q == '+' || *q == '-')
            en = *q++ == '-';
        if (isdigit((unsigned char)*q)) {
            long x = 0;
            while (isdigit((unsigned char)*q)) {
                if (x < 100000)
                    x = x * 10 + (*q - '0');
                q++;
            }
            e += en ? -x : x;
            p = q;
        }
    }
    if (end)
        *end = (char *)p;
    if (m == 0)
        return neg ? -0.0L : 0.0L;
    /* The digits past the 19th only nudge the value off a tie. */
    long double v = (long double)m + (sticky ? 0.25L : 0.0L);
    if (e < -4950 + 19)
        r = 0;
    else if (e > 4932)
        r = __builtin_infl();
    else if (e >= 0)
        r = v * pow10l_(e);
    else if (e > -4900)
        r = v / pow10l_(-e);
    else
        r = (v / pow10l_(4000)) / pow10l_(-e - 4000);
    return neg ? -r : r;
}

double strtod(const char *__restrict s, char **__restrict end) {
    long double r = strtold(s, end);
    /* Fast path, exact: up to 15 digits and 10^22 or less. */
    double d = (double)r;
    if ((d == __builtin_inf() || d == -__builtin_inf()) && r == r && r != __builtin_infl() && r != -__builtin_infl())
        errno = ERANGE;
    else if (d == 0 && r != 0)
        errno = ERANGE;
    return d;
}

float strtof(const char *__restrict s, char **__restrict end) {
    long double r = strtold(s, end);
    float f = (float)r;
    if ((f == __builtin_inff() || f == -__builtin_inff()) && r == r && r != __builtin_infl() && r != -__builtin_infl())
        errno = ERANGE;
    else if (f == 0 && r != 0)
        errno = ERANGE;
    return f;
}
