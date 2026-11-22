#ifndef _STDLIB_H
#define _STDLIB_H
#include "_rs2libc.h"
_RS2_BEGIN
#define EXIT_SUCCESS 0
#define EXIT_FAILURE 1
#define RAND_MAX 0x7fffffff
#define MB_CUR_MAX 1
typedef struct { int quot, rem; } div_t;
typedef struct { long quot, rem; } ldiv_t;
typedef struct { long long quot, rem; } lldiv_t;

void *malloc(size_t);
void *calloc(size_t, size_t);
void *realloc(void *, size_t);
void *reallocarray(void *, size_t, size_t);
void free(void *);
void *aligned_alloc(size_t, size_t);
int posix_memalign(void **, size_t, size_t);

_RS2_NORETURN void abort(void);
int atexit(void (*)(void));
int at_quick_exit(void (*)(void));
/* The Itanium C++ ABI's exit list (a module's destructors). */
int __cxa_atexit(void (*)(void *), void *, void *);
void __cxa_finalize(void *);
_RS2_NORETURN void exit(int);
_RS2_NORETURN void _Exit(int);
_RS2_NORETURN void _exit(int);
_RS2_NORETURN void quick_exit(int);
char *getenv(const char *);
int putenv(char *);
int setenv(const char *, const char *, int);
int unsetenv(const char *);
int system(const char *);

int atoi(const char *);
long atol(const char *);
long long atoll(const char *);
double atof(const char *);
long strtol(const char *__restrict, char **__restrict, int);
long long strtoll(const char *__restrict, char **__restrict, int);
unsigned long strtoul(const char *__restrict, char **__restrict, int);
unsigned long long strtoull(const char *__restrict, char **__restrict, int);
double strtod(const char *__restrict, char **__restrict);
float strtof(const char *__restrict, char **__restrict);
long double strtold(const char *__restrict, char **__restrict);

void qsort(void *, size_t, size_t, int (*)(const void *, const void *));
void *bsearch(const void *, const void *, size_t, size_t, int (*)(const void *, const void *));
int abs(int);
long labs(long);
long long llabs(long long);
div_t div(int, int);
ldiv_t ldiv(long, long);
lldiv_t lldiv(long long, long long);
int rand(void);
void srand(unsigned);
int rand_r(unsigned *);

int mblen(const char *, size_t);
int mbtowc(wchar_t *__restrict, const char *__restrict, size_t);
int wctomb(char *, wchar_t);
size_t mbstowcs(wchar_t *__restrict, const char *__restrict, size_t);
size_t wcstombs(char *__restrict, const wchar_t *__restrict, size_t);

/* Common in OS/2 and DOS compilers. */
char *itoa(int, char *, int);
char *ltoa(long, char *, int);
char *ultoa(unsigned long, char *, int);
char *_fullpath(char *, const char *, size_t);
char *realpath(const char *__restrict, char *__restrict);
char *mktemp(char *);
int mkstemp(char *);
_RS2_END
#endif
