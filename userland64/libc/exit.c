/* exit, atexit, abort, assert. */
#include "libc.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <signal.h>

/* atexit: up to 32 at first, more as needed. */
static void (*exit_fns[32])(void);
static void (**exit_list)(void) = exit_fns;
static int exit_count, exit_cap = 32;
static void (*quick_fns[32])(void);
static int quick_count;
static _rs2_lock_t exit_lock;

int atexit(void (*fn)(void)) {
    _rs2_lock(&exit_lock);
    if (exit_count == exit_cap) {
        void (**n)(void) = malloc(sizeof(*n) * exit_cap * 2);
        if (!n) {
            _rs2_unlock(&exit_lock);
            return -1;
        }
        memcpy(n, exit_list, sizeof(*n) * exit_cap);
        if (exit_list != exit_fns)
            free(exit_list);
        exit_list = n;
        exit_cap *= 2;
    }
    exit_list[exit_count++] = fn;
    _rs2_unlock(&exit_lock);
    return 0;
}

int at_quick_exit(void (*fn)(void)) {
    if (quick_count == 32)
        return -1;
    quick_fns[quick_count++] = fn;
    return 0;
}

void exit(int code) {
    _rs2_lock(&exit_lock);
    while (exit_count > 0)
        exit_list[--exit_count]();
    _rs2_unlock(&exit_lock);
    _rs2_stdio_exit();
    DosExit(EXIT_PROCESS, (ULONG)code);
    __builtin_unreachable();
}

void quick_exit(int code) {
    while (quick_count > 0)
        quick_fns[--quick_count]();
    _Exit(code);
}

void _Exit(int code) {
    DosExit(EXIT_PROCESS, (ULONG)code);
    __builtin_unreachable();
}

void _exit(int code) {
    _Exit(code);
}

void abort(void) {
    raise(SIGABRT);
    static const char msg[] = "Abnormal program termination\r\n";
    ULONG n;
    DosWrite(2, msg, sizeof msg - 1, &n);
    DosExit(EXIT_PROCESS, 3);
    __builtin_unreachable();
}

void __assert_fail(const char *expr, const char *file, int line, const char *func) {
    fprintf(stderr, "Assertion failed: %s, file %s, line %d, function %s\n", expr, file, line, func);
    fflush(stderr);
    abort();
}
