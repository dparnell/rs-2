/* Shared definitions of rs2's 64-bit C library (docs/LIBC64.md). The C
 * library is LP64: int is 32 bits; long, pointers and size_t are 64. */
#ifndef _RS2LIBC_H
#define _RS2LIBC_H
#if defined(__cplusplus)
#define _RS2_BEGIN extern "C" {
#define _RS2_END }
#define _RS2_NORETURN [[noreturn]]
#else
#define _RS2_BEGIN
#define _RS2_END
#define _RS2_NORETURN _Noreturn
#endif
#ifndef NULL
#ifdef __cplusplus
#define NULL __null
#else
#define NULL ((void *)0)
#endif
#endif
#ifndef _SIZE_T_DEFINED
#define _SIZE_T_DEFINED
typedef __SIZE_TYPE__ size_t;
#endif
#ifndef _WCHAR_T_DEFINED
#define _WCHAR_T_DEFINED
#ifndef __cplusplus
typedef __WCHAR_TYPE__ wchar_t;
#endif
#endif
/* Data in the C library: with LIBC64.DLL (cc64.sh's default; -D
 * _RS2_LIBC_DLL), imported from it by name. */
#if defined(_RS2_LIBC_DLL) && !defined(_RS2_BUILDING_LIBC)
#define _RS2_DATA(n) __asm__("LIBC64." #n)
#else
#define _RS2_DATA(n)
#endif
#define _RS2_PRINTF(f, a) __attribute__((format(printf, f, a)))
#define _RS2_SCANF(f, a) __attribute__((format(scanf, f, a)))
#endif
