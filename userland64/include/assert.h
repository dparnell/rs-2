/* No include guard: NDEBUG may change between inclusions. */
#include "_rs2libc.h"
#undef assert
#ifdef NDEBUG
#define assert(e) ((void)0)
#else
_RS2_BEGIN
_RS2_NORETURN void __assert_fail(const char *expr, const char *file, int line, const char *func);
_RS2_END
#define assert(e) ((e) ? (void)0 : __assert_fail(#e, __FILE__, __LINE__, __func__))
#endif
#if !defined(__cplusplus) && !defined(static_assert)
#define static_assert _Static_assert
#endif
