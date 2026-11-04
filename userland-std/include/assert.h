/* rs2libc: assert (C code built into userland-std programs). */
#ifndef _RS2LIBC_ASSERT_H
#define _RS2LIBC_ASSERT_H

void __assert_fail(const char *expr, const char *file, unsigned int line, const char *func);

#ifdef NDEBUG
#define assert(e) ((void)0)
#else
#define assert(e) ((e) ? (void)0 : __assert_fail(#e, __FILE__, __LINE__, __func__))
#endif

#ifndef __cplusplus
#define static_assert _Static_assert
#endif

#endif
