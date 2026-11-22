/* Start-up and exit: _start, main's arguments, the environment, exit. */
#include "libc.h"
#include <stdlib.h>
#include <string.h>
#include <stdio.h>
#include <signal.h>


/* For programs that want them: the module, the raw blocks. */
HMODULE _rs2_hmod;
PCSZ _rs2_environment;
PCSZ _rs2_cmdline;

/* Splits the arguments as OS/2 C runtimes do: blanks separate them,
 * double quotes group, \" is a quote, backslashes before a quote pair. */
static int split(const char *s, char ***out) {
    size_t len = strlen(s);
    char *buf = malloc(len + 1);
    char **v = malloc(sizeof(char *) * (len / 2 + 2));
    int n = 0;
    char *d = buf;
    while (*s) {
        while (*s == ' ' || *s == '\t')
            s++;
        if (!*s)
            break;
        v[n++] = d;
        int quoted = 0;
        while (*s && (quoted || (*s != ' ' && *s != '\t'))) {
            if (*s == '\\') {
                size_t k = 0;
                while (s[k] == '\\')
                    k++;
                if (s[k] == '"') {
                    for (size_t i = 0; i < k / 2; i++)
                        *d++ = '\\';
                    s += k;
                    if (k & 1)
                        *d++ = *s++;
                } else {
                    while (k--)
                        *d++ = *s++;
                }
                continue;
            }
            if (*s == '"') {
                quoted = !quoted;
                s++;
                continue;
            }
            *d++ = *s++;
        }
        *d++ = 0;
    }
    v[n] = 0;
    *out = v;
    return n;
}

/* What a program's _start (crt1.c) hands over: its main and its own
 * constructor and destructor lists. The library may be in a DLL. */
static void (**fini_start)(void), (**fini_end)(void);

static void run_fini(void) {
    for (void (**f)(void) = fini_end; f > fini_start;)
        (*--f)();
}

void __rs2_start(const struct _rs2_startup *st) {
    PCSZ env = st->env, cmdline = st->cmdline;
    _rs2_hmod = st->hmod;
    _rs2_environment = env;
    _rs2_cmdline = cmdline;
    DosQuerySysInfo(QSV_MS_COUNT, QSV_MS_COUNT, &_rs2_start_ms, sizeof _rs2_start_ms);
    /* The environment: pointers into the block (setenv adds to them). */
    int ne = 0;
    for (const char *e = (const char *)env; *e; e += strlen(e) + 1)
        ne++;
    environ = malloc(sizeof(char *) * (ne + 1));
    ne = 0;
    for (const char *e = (const char *)env; *e; e += strlen(e) + 1)
        environ[ne++] = (char *)e;
    environ[ne] = 0;
    _rs2_environ_owned = 1;
    /* argv[0]: the program as started; then the arguments. */
    const char *prog = (const char *)cmdline;
    char **args;
    int argc = split(prog + strlen(prog) + 1, &args);
    char **argv = malloc(sizeof(char *) * (argc + 2));
    argv[0] = (char *)prog;
    memcpy(argv + 1, args, sizeof(char *) * (argc + 1));
    free(args);
    _rs2_argv = argv;
    /* Signals and faults go through an exception handler whose record
     * lives in this frame, below main. */
    struct {
        void *prev;
        void *handler;
    } reg;
    extern ULONG _rs2_xcpt_handler(PEXCEPTIONREPORTRECORD, PEXCEPTIONREGISTRATIONRECORD, PCONTEXTRECORD, PVOID);
    reg.handler = (void *)_rs2_xcpt_handler;
    DosSetExceptionHandler((PEXCEPTIONREGISTRATIONRECORD)&reg);
    /* Constructors now, destructors at exit. */
    for (void (**f)(void) = st->init_start; f < st->init_end; f++)
        (*f)();
    fini_start = st->fini_start;
    fini_end = st->fini_end;
    atexit(run_fini);
    exit(st->main(argc + 1, argv, environ));
}
