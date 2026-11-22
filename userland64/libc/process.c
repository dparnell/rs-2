/* Running programs: system, spawn, wait. */
#include "libc.h"
#include <process.h>
#include <stdlib.h>
#include <string.h>
#include <stdarg.h>
#include <unistd.h>

/* "prog\0arg1 arg2 ...\0\0", quoting arguments with blanks or quotes so
 * that the child's C runtime splits them back the same way. */
char *_rs2_build_args(const char *prog, char *const argv[]) {
    size_t len = strlen(prog) + 4;
    for (int i = 1; argv && argv[i]; i++)
        len += strlen(argv[i]) * 2 + 3;
    char *b = malloc(len), *p = b;
    if (!b)
        return 0;
    p = stpcpy(p, prog) + 1;
    for (int i = 1; argv && argv[i]; i++) {
        const char *a = argv[i];
        if (i > 1)
            *p++ = ' ';
        int q = !*a || strpbrk(a, " \t\"");
        if (q)
            *p++ = '"';
        for (; *a; a++) {
            if (*a == '"' || (*a == '\\' && q && (a[1] == '"' || !a[1])))
                *p++ = '\\';
            *p++ = *a;
        }
        if (q)
            *p++ = '"';
    }
    *p++ = 0;
    *p = 0;
    return b;
}

static int run(int mode, const char *path, char *const argv[]) {
    char *args = _rs2_build_args(argv && argv[0] ? argv[0] : path, argv);
    char *env = _rs2_env_block();
    if (!args || !env) {
        free(args);
        free(env);
        errno = ENOMEM;
        return -1;
    }
    char failed[260];
    RESULTCODES rc;
    APIRET r = DosExecPgm((PCHAR)failed, sizeof failed, mode == P_WAIT ? EXEC_SYNC : EXEC_ASYNCRESULT, (PCSZ)args,
                          (PCSZ)env, &rc, (PCSZ)path);
    free(args);
    free(env);
    if (r)
        return _rs2_oserr(r);
    return mode == P_WAIT ? (int)rc.codeResult : (int)rc.codeTerminate;
}

int spawnv(int mode, const char *path, char *const argv[]) {
    return run(mode, path, argv);
}

/* Finds a program along PATH, adding .EXE when the name has no extension. */
static int search(const char *file, char *out, size_t n) {
    char name[264];
    const char *base = file;
    for (const char *p = file; *p; p++)
        if (*p == '\\' || *p == '/' || *p == ':')
            base = p + 1;
    if (strlen(file) > 255) {
        errno = ENAMETOOLONG;
        return -1;
    }
    strcpy(name, file);
    if (!strchr(base, '.'))
        strcat(name, ".EXE");
    if (base != file) {
        strlcpy(out, name, n);
        return 0;
    }
    if (access(name, F_OK) == 0) {
        strlcpy(out, name, n);
        return 0;
    }
    APIRET rc = DosSearchPath(2 /* SEARCH_ENVIRONMENT */ | 1 /* SEARCH_CUR_DIRECTORY */, (PCSZ) "PATH", (PCSZ)name,
                              (PBYTE)out, (ULONG)n);
    return rc ? _rs2_oserr(rc) : 0;
}

int spawnvp(int mode, const char *file, char *const argv[]) {
    char path[264];
    if (search(file, path, sizeof path))
        return -1;
    return run(mode, path, argv);
}

static int spawnl_(int mode, const char *path, const char *arg0, va_list ap, int (*fn)(int, const char *, char *const[])) {
    char *argv[256];
    int n = 0;
    argv[n++] = (char *)arg0;
    char *a;
    while (n < 255 && (a = va_arg(ap, char *)))
        argv[n++] = a;
    argv[n] = 0;
    return fn(mode, path, argv);
}

int spawnl(int mode, const char *path, const char *arg0, ...) {
    va_list ap;
    va_start(ap, arg0);
    int r = spawnl_(mode, path, arg0, ap, spawnv);
    va_end(ap);
    return r;
}

int spawnlp(int mode, const char *file, const char *arg0, ...) {
    va_list ap;
    va_start(ap, arg0);
    int r = spawnl_(mode, file, arg0, ap, spawnvp);
    va_end(ap);
    return r;
}

int cwait(int *status, int pid, int action) {
    RESULTCODES rc;
    PID done;
    APIRET r = DosWaitChild(action ? DCWA_PROCESSTREE : DCWA_PROCESS, DCWW_WAIT, &rc, &done, (PID)pid);
    if (r)
        return _rs2_oserr(r);
    if (status)
        *status = (int)(rc.codeResult << 8 | (rc.codeTerminate & 0xff));
    return (int)done;
}

int wait(int *status) {
    return cwait(status, 0, 0);
}

int system(const char *cmd) {
    const char *shell = getenv("COMSPEC");
    if (!shell)
        shell = "CMD.EXE";
    if (!cmd)
        return access(shell, F_OK) == 0 || 1;
    size_t n = strlen(shell), c = strlen(cmd);
    char *args = malloc(n + c + 8);
    char *env = _rs2_env_block();
    if (!args || !env) {
        free(args);
        free(env);
        errno = ENOMEM;
        return -1;
    }
    char *p = stpcpy(args, shell) + 1;
    p = stpcpy(p, "/c ");
    p = stpcpy(p, cmd) + 1;
    *p = 0;
    char failed[260];
    RESULTCODES rc;
    APIRET r = DosExecPgm((PCHAR)failed, sizeof failed, EXEC_SYNC, (PCSZ)args, (PCSZ)env, &rc, (PCSZ)shell);
    free(args);
    free(env);
    if (r)
        return _rs2_oserr(r);
    return (int)rc.codeResult;
}

static PPIB pib(void) {
    PTIB t;
    PPIB p;
    DosGetInfoBlocks(&t, &p);
    return p;
}

pid_t getpid(void) {
    return (pid_t)pib()->pib_ulpid;
}

pid_t getppid(void) {
    return (pid_t)pib()->pib_ulppid;
}
