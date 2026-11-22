/* C start-up code for rs2's 64-bit programs (docs/API64.md).
 *
 * The loader calls _start(hmod, 0, env, cmdline) as a System V function;
 * this splits the command line ("program\0arguments\0") into argc/argv as
 * C wants it and calls main(argc, argv, envp). Returning from _start ends
 * the process with main's result. Also the few routines clang may call on
 * its own (memcpy, memset, memmove, memcmp). Built with -fno-builtin.
 */

#include <os2.h>

int main(int argc, char **argv, char **envp);

#define MAX_ARGS 256
#define MAX_ENV 1024

static char *argv_[MAX_ARGS + 1];
static char *envp_[MAX_ENV + 1];
static char args_copy[4096];

/* The module handle and the raw blocks, for programs that want them. */
HMODULE _rs2_hmod;
PCSZ _rs2_environment;
PCSZ _rs2_cmdline;

static int split(char *s, char **out, int max) {
    int n = 0;
    while (*s && n < max) {
        while (*s == ' ' || *s == '\t')
            s++;
        if (!*s)
            break;
        char *d = s;
        out[n++] = d;
        int quoted = 0;
        while (*s && (quoted || (*s != ' ' && *s != '\t'))) {
            if (*s == '"') {
                quoted = !quoted;
                s++;
                continue;
            }
            *d++ = *s++;
        }
        if (*s)
            s++;
        *d = 0;
    }
    out[n] = 0;
    return n;
}

unsigned long _start(ULONG hmod, ULONG reserved, PCSZ env, PCSZ cmdline) {
    (void)reserved;
    _rs2_hmod = hmod;
    _rs2_environment = env;
    _rs2_cmdline = cmdline;
    /* argv[0]: the program as started; then the arguments, split. */
    const char *prog = (const char *)cmdline;
    const char *rest = prog;
    while (*rest)
        rest++;
    rest++;
    unsigned long i = 0;
    while (rest[i] && i < sizeof args_copy - 1) {
        args_copy[i] = rest[i];
        i++;
    }
    args_copy[i] = 0;
    argv_[0] = (char *)prog;
    int argc = 1 + split(args_copy, argv_ + 1, MAX_ARGS - 1);
    int n = 0;
    for (const char *e = (const char *)env; *e && n < MAX_ENV; n++) {
        envp_[n] = (char *)e;
        while (*e)
            e++;
        e++;
    }
    envp_[n] = 0;
    return (unsigned long)main(argc, argv_, envp_);
}

void *memcpy(void *d, const void *s, size_t n) {
    volatile unsigned char *dd = d;
    const volatile unsigned char *ss = s;
    while (n--)
        *dd++ = *ss++;
    return d;
}

void *memmove(void *d, const void *s, size_t n) {
    volatile unsigned char *dd = d;
    const volatile unsigned char *ss = s;
    if (dd < ss) {
        while (n--)
            *dd++ = *ss++;
    } else {
        dd += n;
        ss += n;
        while (n--)
            *--dd = *--ss;
    }
    return d;
}

void *memset(void *d, int c, size_t n) {
    volatile unsigned char *dd = d;
    while (n--)
        *dd++ = (unsigned char)c;
    return d;
}

int memcmp(const void *a, const void *b, size_t n) {
    const unsigned char *x = a, *y = b;
    for (; n; n--, x++, y++)
        if (*x != *y)
            return *x - *y;
    return 0;
}
