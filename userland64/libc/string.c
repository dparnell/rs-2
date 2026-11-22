/* <string.h>, <strings.h>. */
#include "libc.h"
#include <string.h>
#include <strings.h>
#include <stdlib.h>
#include <ctype.h>

void *memcpy(void *__restrict d, const void *__restrict s, size_t n) {
    void *r = d;
    __asm__ volatile("rep movsb" : "+D"(d), "+S"(s), "+c"(n) : : "memory");
    return r;
}

void *memmove(void *d, const void *s, size_t n) {
    if ((uintptr_t)d - (uintptr_t)s >= n)
        return memcpy(d, s, n);
    /* Overlapping, destination above: copy backwards. */
    void *r = d;
    unsigned char *dd = (unsigned char *)d + n - 1;
    const unsigned char *ss = (const unsigned char *)s + n - 1;
    __asm__ volatile("std; rep movsb; cld" : "+D"(dd), "+S"(ss), "+c"(n) : : "memory");
    return r;
}

void *memset(void *d, int c, size_t n) {
    void *r = d;
    __asm__ volatile("rep stosb" : "+D"(d), "+c"(n) : "a"(c) : "memory");
    return r;
}

int memcmp(const void *a, const void *b, size_t n) {
    const unsigned char *x = a, *y = b;
    for (; n; n--, x++, y++)
        if (*x != *y)
            return *x - *y;
    return 0;
}

/* clang calls bcmp for memcmp(...) == 0. */
int bcmp(const void *a, const void *b, size_t n) {
    return memcmp(a, b, n);
}

void *memchr(const void *s, int c, size_t n) {
    const unsigned char *p = s;
    for (; n; n--, p++)
        if (*p == (unsigned char)c)
            return (void *)p;
    return 0;
}

void *memrchr(const void *s, int c, size_t n) {
    const unsigned char *p = (const unsigned char *)s + n;
    while (n--)
        if (*--p == (unsigned char)c)
            return (void *)p;
    return 0;
}

void *memccpy(void *__restrict d, const void *__restrict s, int c, size_t n) {
    unsigned char *dd = d;
    const unsigned char *ss = s;
    for (; n; n--) {
        if ((*dd++ = *ss++) == (unsigned char)c)
            return dd;
    }
    return 0;
}

size_t strlen(const char *s) {
    const char *p = s;
    while (*p)
        p++;
    return p - s;
}

size_t strnlen(const char *s, size_t n) {
    size_t i = 0;
    while (i < n && s[i])
        i++;
    return i;
}

char *strcpy(char *__restrict d, const char *__restrict s) {
    char *r = d;
    while ((*d++ = *s++))
        ;
    return r;
}

char *stpcpy(char *__restrict d, const char *__restrict s) {
    while ((*d = *s++))
        d++;
    return d;
}

char *strncpy(char *__restrict d, const char *__restrict s, size_t n) {
    size_t i = 0;
    for (; i < n && s[i]; i++)
        d[i] = s[i];
    for (; i < n; i++)
        d[i] = 0;
    return d;
}

char *strcat(char *__restrict d, const char *__restrict s) {
    strcpy(d + strlen(d), s);
    return d;
}

char *strncat(char *__restrict d, const char *__restrict s, size_t n) {
    char *p = d + strlen(d);
    while (n-- && *s)
        *p++ = *s++;
    *p = 0;
    return d;
}

size_t strlcpy(char *__restrict d, const char *__restrict s, size_t n) {
    size_t len = strlen(s);
    if (n) {
        size_t c = len < n - 1 ? len : n - 1;
        memcpy(d, s, c);
        d[c] = 0;
    }
    return len;
}

size_t strlcat(char *__restrict d, const char *__restrict s, size_t n) {
    size_t dl = strnlen(d, n);
    if (dl == n)
        return n + strlen(s);
    return dl + strlcpy(d + dl, s, n - dl);
}

int strcmp(const char *a, const char *b) {
    while (*a && *a == *b)
        a++, b++;
    return (unsigned char)*a - (unsigned char)*b;
}

int strncmp(const char *a, const char *b, size_t n) {
    for (; n; n--, a++, b++) {
        if (*a != *b)
            return (unsigned char)*a - (unsigned char)*b;
        if (!*a)
            return 0;
    }
    return 0;
}

int strcoll(const char *a, const char *b) {
    return strcmp(a, b);
}

size_t strxfrm(char *__restrict d, const char *__restrict s, size_t n) {
    size_t len = strlen(s);
    if (len < n)
        strcpy(d, s);
    return len;
}

int strcasecmp(const char *a, const char *b) {
    int x, y;
    do {
        x = tolower((unsigned char)*a++);
        y = tolower((unsigned char)*b++);
    } while (x && x == y);
    return x - y;
}

int strncasecmp(const char *a, const char *b, size_t n) {
    int x = 0, y = 0;
    while (n--) {
        x = tolower((unsigned char)*a++);
        y = tolower((unsigned char)*b++);
        if (!x || x != y)
            break;
    }
    return x - y;
}

int stricmp(const char *a, const char *b) {
    return strcasecmp(a, b);
}

int strnicmp(const char *a, const char *b, size_t n) {
    return strncasecmp(a, b, n);
}

int ffs(int i) {
    return __builtin_ffs(i);
}

char *strchr(const char *s, int c) {
    for (;; s++) {
        if (*s == (char)c)
            return (char *)s;
        if (!*s)
            return 0;
    }
}

char *strrchr(const char *s, int c) {
    const char *r = 0;
    for (;; s++) {
        if (*s == (char)c)
            r = s;
        if (!*s)
            return (char *)r;
    }
}

size_t strspn(const char *s, const char *set) {
    size_t n = 0;
    while (s[n] && strchr(set, s[n]))
        n++;
    return n;
}

size_t strcspn(const char *s, const char *set) {
    size_t n = 0;
    while (s[n] && !strchr(set, s[n]))
        n++;
    return n;
}

char *strpbrk(const char *s, const char *set) {
    s += strcspn(s, set);
    return *s ? (char *)s : 0;
}

char *strstr(const char *h, const char *n) {
    size_t nl = strlen(n);
    if (!nl)
        return (char *)h;
    for (; *h; h++)
        if (*h == *n && !strncmp(h, n, nl))
            return (char *)h;
    return 0;
}

char *strtok_r(char *__restrict s, const char *__restrict delim, char **__restrict save) {
    if (!s)
        s = *save;
    if (!s)
        return 0;
    s += strspn(s, delim);
    if (!*s) {
        *save = 0;
        return 0;
    }
    char *e = s + strcspn(s, delim);
    if (*e)
        *e++ = 0;
    else
        e = 0;
    *save = e;
    return s;
}

char *strtok(char *__restrict s, const char *__restrict delim) {
    return strtok_r(s, delim, &_rs2_self()->strtok_next);
}

char *strsep(char **sp, const char *delim) {
    char *s = *sp;
    if (!s)
        return 0;
    char *e = s + strcspn(s, delim);
    if (*e)
        *e++ = 0, *sp = e;
    else
        *sp = 0;
    return s;
}

char *strdup(const char *s) {
    size_t n = strlen(s) + 1;
    char *d = malloc(n);
    return d ? memcpy(d, s, n) : 0;
}

char *strndup(const char *s, size_t n) {
    n = strnlen(s, n);
    char *d = malloc(n + 1);
    if (!d)
        return 0;
    memcpy(d, s, n);
    d[n] = 0;
    return d;
}

char *strupr(char *s) {
    for (char *p = s; *p; p++)
        *p = (char)toupper((unsigned char)*p);
    return s;
}

char *strlwr(char *s) {
    for (char *p = s; *p; p++)
        *p = (char)tolower((unsigned char)*p);
    return s;
}

char *strrev(char *s) {
    size_t n = strlen(s);
    for (size_t i = 0; i < n / 2; i++) {
        char t = s[i];
        s[i] = s[n - 1 - i];
        s[n - 1 - i] = t;
    }
    return s;
}
