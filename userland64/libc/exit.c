/* exit, atexit, abort, assert. */
#include "libc.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <signal.h>

/* The exit list: atexit functions and __cxa_atexit ones (a DLL's
 * destructors, C++ objects), run last first. Each may belong to a module
 * (dso): __cxa_finalize runs and drops a module's own when it is freed. */
struct exit_fn {
    void (*fn)(void *);
    void *arg;
    void *dso;
    int plain;   /* atexit: fn takes no argument */
};
static struct exit_fn exit_fns[32];
static struct exit_fn *exit_list = exit_fns;
static int exit_count, exit_cap = 32;
static void (*quick_fns[32])(void);
static int quick_count;
static _rs2_lock_t exit_lock;

static int add(void (*fn)(void *), void *arg, void *dso, int plain) {
    _rs2_lock(&exit_lock);
    if (exit_count == exit_cap) {
        struct exit_fn *n = malloc(sizeof(*n) * exit_cap * 2);
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
    exit_list[exit_count++] = (struct exit_fn){fn, arg, dso, plain};
    _rs2_unlock(&exit_lock);
    return 0;
}

int atexit(void (*fn)(void)) {
    return add((void (*)(void *))fn, 0, 0, 1);
}

int __cxa_atexit(void (*fn)(void *), void *arg, void *dso) {
    return add(fn, arg, dso, 0);
}

static void call(struct exit_fn e) {
    if (e.plain)
        ((void (*)(void))e.fn)();
    else
        e.fn(e.arg);
}

/* Runs (and drops) the functions of module dso, or all of them for 0.
 * The lock is let go around each call: a function may add more. */
void __cxa_finalize(void *dso) {
    _rs2_lock(&exit_lock);
    for (int i = exit_count - 1; i >= 0; i--) {
        if (i >= exit_count || (dso && exit_list[i].dso != dso))
            continue;
        struct exit_fn e = exit_list[i];
        memmove(&exit_list[i], &exit_list[i + 1], sizeof(*exit_list) * (exit_count - i - 1));
        exit_count--;
        _rs2_unlock(&exit_lock);
        call(e);
        _rs2_lock(&exit_lock);
    }
    _rs2_unlock(&exit_lock);
}

int at_quick_exit(void (*fn)(void)) {
    if (quick_count == 32)
        return -1;
    quick_fns[quick_count++] = fn;
    return 0;
}

void exit(int code) {
    __cxa_finalize(0);
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
