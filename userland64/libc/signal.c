/* signal and raise. One exception handler, registered by _start, turns
 * OS/2 exceptions into signals: Ctrl+C and Ctrl+Break (XCPT_SIGNAL) into
 * SIGINT and SIGBREAK, DosKillProcess into SIGTERM, faults into SIGSEGV,
 * SIGILL and SIGFPE. A handler runs after the exception is dismissed (the
 * exception handler sends the thread to a trampoline), so it may return
 * or longjmp out. */
#include "libc.h"
#include <signal.h>
#include <string.h>
#include <stdlib.h>

static volatile __sighandler_t handlers[NSIG];
static int focus;

__sighandler_t signal(int sig, __sighandler_t h) {
    if (sig <= 0 || sig >= NSIG || sig == SIGKILL) {
        errno = EINVAL;
        return SIG_ERR;
    }
    __sighandler_t old = handlers[sig];
    handlers[sig] = h;
    if ((sig == SIGINT || sig == SIGBREAK) && h != SIG_DFL && !focus) {
        /* Ctrl+C reaches the program even when it is not the session's
         * foreground process. */
        ULONG times;
        DosSetSignalExceptionFocus(1, &times);
        focus = 1;
    }
    return old;
}

int raise(int sig) {
    if (sig <= 0 || sig >= NSIG) {
        errno = EINVAL;
        return -1;
    }
    __sighandler_t h = handlers[sig];
    if (h == SIG_IGN)
        return 0;
    if (h == SIG_DFL) {
        if (sig == SIGABRT)
            return 0;   /* abort() goes on to end the process */
        static const char msg[] = "Program ended by a signal\r\n";
        ULONG n;
        DosWrite(2, msg, sizeof msg - 1, &n);
        DosExit(EXIT_PROCESS, 3);
    }
    h(sig);
    return 0;
}

extern void _rs2_sigtramp(void);

/* Context record offsets (64-bit). */
#define CTX_REGS 8
#define CTX_RIP 136
#define CTX_RFLAGS 144

ULONG _rs2_xcpt_handler(PEXCEPTIONREPORTRECORD rep, PEXCEPTIONREGISTRATIONRECORD reg, PCONTEXTRECORD ctx, PVOID p) {
    (void)reg;
    (void)p;
    if (rep->fHandlerFlags & 0x06)   /* unwinding */
        return XCPT_CONTINUE_SEARCH;
    int sig = 0, async = 0;
    switch (rep->ExceptionNum) {
    case XCPT_SIGNAL:
        async = 1;
        switch (rep->ExceptionInfo[0]) {
        case XCPT_SIGNAL_INTR: sig = SIGINT; break;
        case XCPT_SIGNAL_BREAK: sig = SIGBREAK; break;
        case XCPT_SIGNAL_KILLPROC: sig = SIGTERM; break;
        }
        break;
    case XCPT_ACCESS_VIOLATION: case XCPT_ARRAY_BOUNDS_EXCEEDED: sig = SIGSEGV; break;
    case XCPT_ILLEGAL_INSTRUCTION: case XCPT_PRIVILEGED_INSTRUCTION: sig = SIGILL; break;
    case XCPT_INTEGER_DIVIDE_BY_ZERO: case XCPT_INTEGER_OVERFLOW:
    case XCPT_FLOAT_DIVIDE_BY_ZERO: case XCPT_FLOAT_INVALID_OPERATION: case XCPT_FLOAT_OVERFLOW:
    case XCPT_FLOAT_UNDERFLOW: case XCPT_FLOAT_INEXACT_RESULT: case XCPT_FLOAT_DENORMAL_OPERAND:
    case XCPT_FLOAT_STACK_CHECK:
        sig = SIGFPE;
        break;
    }
    if (!sig)
        return XCPT_CONTINUE_SEARCH;
    __sighandler_t h = handlers[sig];
    if (h == SIG_DFL)
        return XCPT_CONTINUE_SEARCH;
    if (async)
        DosAcknowledgeSignalException((ULONG)rep->ExceptionInfo[0]);
    if (h == SIG_IGN)
        return async ? XCPT_CONTINUE_EXECUTION : XCPT_CONTINUE_SEARCH;
    /* Save the interrupted registers and send the thread to the
     * trampoline, below the interrupted code's red zone. */
    unsigned char *c = (unsigned char *)ctx;
    unsigned long *save = _rs2_self()->sigregs;
    memcpy(save, c + CTX_REGS, 16 * 8);
    memcpy(save + 16, c + CTX_RIP, 8);
    memcpy(save + 17, c + CTX_RFLAGS, 8);
    unsigned long *regs = (unsigned long *)(c + CTX_REGS);
    unsigned long rsp = regs[7];
    regs[7] = (rsp - 128 - 64) & ~15UL;   /* rsp */
    regs[5] = (unsigned long)sig;        /* rdi */
    regs[4] = (unsigned long)h;          /* rsi */
    regs[3] = (unsigned long)save;       /* rdx */
    unsigned long rip = (unsigned long)_rs2_sigtramp;
    memcpy(c + CTX_RIP, &rip, 8);
    return XCPT_CONTINUE_EXECUTION;
}
