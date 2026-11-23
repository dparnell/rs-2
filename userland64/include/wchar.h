#ifndef _WCHAR_H
#define _WCHAR_H
#include <stddef.h>
#include <stdarg.h>
#include <stdio.h>
#include <time.h>
_RS2_BEGIN
/* Multibyte text is single bytes ("C" locale): byte n is wide character n. */
#ifndef _WINT_T_DEFINED
#define _WINT_T_DEFINED
typedef unsigned int wint_t;
#endif
#ifndef _WCTYPE_T_DEFINED
#define _WCTYPE_T_DEFINED
typedef unsigned long wctype_t;
#endif
#ifndef _MBSTATE_T_DEFINED
#define _MBSTATE_T_DEFINED
typedef struct { unsigned __state[2]; } mbstate_t;
#endif
#define WEOF ((wint_t)-1)
#ifndef WCHAR_MIN
#define WCHAR_MIN (-2147483647 - 1)
#define WCHAR_MAX 2147483647
#endif
wint_t btowc(int);
int wctob(wint_t);
int mbsinit(const mbstate_t *);
size_t mbrlen(const char *__restrict, size_t, mbstate_t *__restrict);
size_t mbrtowc(wchar_t *__restrict, const char *__restrict, size_t, mbstate_t *__restrict);
size_t wcrtomb(char *__restrict, wchar_t, mbstate_t *__restrict);
size_t mbsrtowcs(wchar_t *__restrict, const char **__restrict, size_t, mbstate_t *__restrict);
size_t wcsrtombs(char *__restrict, const wchar_t **__restrict, size_t, mbstate_t *__restrict);
size_t mbsnrtowcs(wchar_t *__restrict, const char **__restrict, size_t, size_t, mbstate_t *__restrict);
size_t wcsnrtombs(char *__restrict, const wchar_t **__restrict, size_t, size_t, mbstate_t *__restrict);
size_t wcslen(const wchar_t *);
size_t wcsnlen(const wchar_t *, size_t);
wchar_t *wcscpy(wchar_t *__restrict, const wchar_t *__restrict);
wchar_t *wcsncpy(wchar_t *__restrict, const wchar_t *__restrict, size_t);
wchar_t *wcscat(wchar_t *__restrict, const wchar_t *__restrict);
wchar_t *wcsncat(wchar_t *__restrict, const wchar_t *__restrict, size_t);
int wcscmp(const wchar_t *, const wchar_t *);
int wcsncmp(const wchar_t *, const wchar_t *, size_t);
int wcscoll(const wchar_t *, const wchar_t *);
size_t wcsxfrm(wchar_t *__restrict, const wchar_t *__restrict, size_t);
wchar_t *wcschr(const wchar_t *, wchar_t);
wchar_t *wcsrchr(const wchar_t *, wchar_t);
size_t wcsspn(const wchar_t *, const wchar_t *);
size_t wcscspn(const wchar_t *, const wchar_t *);
wchar_t *wcspbrk(const wchar_t *, const wchar_t *);
wchar_t *wcsstr(const wchar_t *, const wchar_t *);
wchar_t *wcstok(wchar_t *__restrict, const wchar_t *__restrict, wchar_t **__restrict);
wchar_t *wcsdup(const wchar_t *);
wchar_t *wmemcpy(wchar_t *__restrict, const wchar_t *__restrict, size_t);
wchar_t *wmemmove(wchar_t *, const wchar_t *, size_t);
wchar_t *wmemset(wchar_t *, wchar_t, size_t);
int wmemcmp(const wchar_t *, const wchar_t *, size_t);
wchar_t *wmemchr(const wchar_t *, wchar_t, size_t);
long wcstol(const wchar_t *__restrict, wchar_t **__restrict, int);
long long wcstoll(const wchar_t *__restrict, wchar_t **__restrict, int);
unsigned long wcstoul(const wchar_t *__restrict, wchar_t **__restrict, int);
unsigned long long wcstoull(const wchar_t *__restrict, wchar_t **__restrict, int);
double wcstod(const wchar_t *__restrict, wchar_t **__restrict);
float wcstof(const wchar_t *__restrict, wchar_t **__restrict);
long double wcstold(const wchar_t *__restrict, wchar_t **__restrict);
size_t wcsftime(wchar_t *__restrict, size_t, const wchar_t *__restrict, const struct tm *__restrict);
int fwide(FILE *, int);
wint_t fgetwc(FILE *);
wint_t getwc(FILE *);
wint_t getwchar(void);
wint_t ungetwc(wint_t, FILE *);
wint_t fputwc(wchar_t, FILE *);
wint_t putwc(wchar_t, FILE *);
wint_t putwchar(wchar_t);
wchar_t *fgetws(wchar_t *__restrict, int, FILE *__restrict);
int fputws(const wchar_t *__restrict, FILE *__restrict);
int swprintf(wchar_t *__restrict, size_t, const wchar_t *__restrict, ...);
int vswprintf(wchar_t *__restrict, size_t, const wchar_t *__restrict, va_list);
int fwprintf(FILE *__restrict, const wchar_t *__restrict, ...);
int vfwprintf(FILE *__restrict, const wchar_t *__restrict, va_list);
int wprintf(const wchar_t *__restrict, ...);
int vwprintf(const wchar_t *__restrict, va_list);
int swscanf(const wchar_t *__restrict, const wchar_t *__restrict, ...);
int vswscanf(const wchar_t *__restrict, const wchar_t *__restrict, va_list);
int fwscanf(FILE *__restrict, const wchar_t *__restrict, ...);
int vfwscanf(FILE *__restrict, const wchar_t *__restrict, va_list);
int wscanf(const wchar_t *__restrict, ...);
int vwscanf(const wchar_t *__restrict, va_list);
_RS2_END
#endif
