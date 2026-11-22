/* The few compiler support routines clang may call: 128-bit division. */
typedef unsigned __int128 u128;
typedef __int128 i128;

static int clz128(u128 x) {
    unsigned long long hi = (unsigned long long)(x >> 64);
    return hi ? __builtin_clzll(hi) : 64 + __builtin_clzll((unsigned long long)x);
}

static u128 udivmod(u128 n, u128 d, u128 *rem) {
    if (d == 0) {
        __builtin_trap();
    }
    if (n < d) {
        if (rem)
            *rem = n;
        return 0;
    }
    if (!(n >> 64) && !(d >> 64)) {
        unsigned long long a = (unsigned long long)n, b = (unsigned long long)d;
        if (rem)
            *rem = a % b;
        return a / b;
    }
    int shift = clz128(d) - clz128(n);
    u128 q = 0;
    d <<= shift;
    for (; shift >= 0; shift--) {
        q <<= 1;
        if (n >= d) {
            n -= d;
            q |= 1;
        }
        d >>= 1;
    }
    if (rem)
        *rem = n;
    return q;
}

u128 __udivti3(u128 a, u128 b) { return udivmod(a, b, 0); }
u128 __umodti3(u128 a, u128 b) { u128 r; udivmod(a, b, &r); return r; }

i128 __divti3(i128 a, i128 b) {
    int neg = (a < 0) != (b < 0);
    u128 q = udivmod(a < 0 ? -(u128)a : (u128)a, b < 0 ? -(u128)b : (u128)b, 0);
    return neg ? -(i128)q : (i128)q;
}

i128 __modti3(i128 a, i128 b) {
    u128 r;
    udivmod(a < 0 ? -(u128)a : (u128)a, b < 0 ? -(u128)b : (u128)b, &r);
    return a < 0 ? -(i128)r : (i128)r;
}
