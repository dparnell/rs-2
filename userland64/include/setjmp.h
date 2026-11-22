#ifndef _SETJMP_H
#define _SETJMP_H
#include "_rs2libc.h"
_RS2_BEGIN
/* rbx, rbp, r12-r15, rsp, the return address, mxcsr and the x87 control word. */
typedef unsigned long jmp_buf[10];
typedef unsigned long sigjmp_buf[10];
int setjmp(jmp_buf) __attribute__((returns_twice));
_RS2_NORETURN void longjmp(jmp_buf, int);
int _setjmp(jmp_buf) __attribute__((returns_twice));
_RS2_NORETURN void _longjmp(jmp_buf, int);
#define sigsetjmp(b, s) setjmp(b)
#define siglongjmp(b, v) longjmp(b, v)
_RS2_END
#endif
