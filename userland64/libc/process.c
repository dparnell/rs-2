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

static int run_env(int mode, const char *path, char *const argv[], char *env) {
    char *args = _rs2_build_args(argv && argv[0] ? argv[0] : path, argv);
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

static int run(int mode, const char *path, char *const argv[]) {
    return run_env(mode, path, argv, _rs2_env_block());
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

pid_t waitpid(pid_t pid, int *status, int options) {
    RESULTCODES rc;
    PID done;
    APIRET r = DosWaitChild(DCWA_PROCESS, options & 1 /* WNOHANG */ ? DCWW_NOWAIT : DCWW_WAIT, &rc, &done, pid < 0 ? 0 : (PID)pid);
    if (r == 129)   /* ERROR_CHILD_NOT_COMPLETE */
        return 0;
    if (r)
        return _rs2_oserr(r);
    if (status) {
        /* TC_EXIT; TC_HARDERROR, TC_TRAP, TC_KILLPROCESS, TC_EXCEPTION */
        static const unsigned char sig[] = {0, 7, 11, 15, 11};
        *status = rc.codeTerminate ? rc.codeTerminate < 5 ? sig[rc.codeTerminate] : 9 : (int)((rc.codeResult & 0xff) << 8);
    }
    return (pid_t)done;
}

struct rusage;
pid_t wait4(pid_t pid, int *status, int options, struct rusage *ru) {
    (void)ru;
    return waitpid(pid, status, options);
}

/* posix_spawn: the child inherits the parent's handles, so the file
 * actions are done here around DosExecPgm, and undone. */
#include <spawn.h>
#include <fcntl.h>

struct action {
    int kind, fd, from, flags;
    mode_t mode;
    char *path;
};

static int add(posix_spawn_file_actions_t *a, struct action x) {
    struct action *v = realloc(a->actions, (a->count + 1) * sizeof x);
    if (!v)
        return ENOMEM;
    a->actions = v;
    v[a->count++] = x;
    return 0;
}

int posix_spawn_file_actions_init(posix_spawn_file_actions_t *a) {
    a->count = 0;
    a->actions = 0;
    return 0;
}
int posix_spawn_file_actions_destroy(posix_spawn_file_actions_t *a) {
    struct action *v = a->actions;
    for (int i = 0; i < a->count; i++)
        free(v[i].path);
    free(v);
    return posix_spawn_file_actions_init(a);
}
int posix_spawn_file_actions_addopen(posix_spawn_file_actions_t *__restrict a, int fd, const char *__restrict path, int flags, mode_t mode) {
    return add(a, (struct action){0, fd, 0, flags, mode, strdup(path)});
}
int posix_spawn_file_actions_adddup2(posix_spawn_file_actions_t *a, int fd, int to) {
    return add(a, (struct action){1, to, fd, 0, 0, 0});
}
int posix_spawn_file_actions_addclose(posix_spawn_file_actions_t *a, int fd) {
    return add(a, (struct action){2, fd, 0, 0, 0, 0});
}
int posix_spawnattr_init(posix_spawnattr_t *a) { a->flags = 0; return 0; }
int posix_spawnattr_destroy(posix_spawnattr_t *a) { (void)a; return 0; }
int posix_spawnattr_setflags(posix_spawnattr_t *a, short f) { a->flags = f; return 0; }

static int spawn(pid_t *pid, const char *path, const posix_spawn_file_actions_t *fa, char *const *argv, char *const *envp, int find) {
    static _rs2_lock_t lock;
    char found[264];
    if (find ? search(path, found, sizeof found) : access(path, F_OK))
        return errno;
    char *env;
    if (envp) {
        size_t n = 2;
        for (char *const *e = envp; *e; e++)
            n += strlen(*e) + 1;
        char *p = env = calloc(1, n);
        for (char *const *e = envp; p && *e; e++)
            p = stpcpy(p, *e) + 1;
    } else {
        env = _rs2_env_block();
    }
    _rs2_lock(&lock);
    struct action *v = fa ? fa->actions : 0;
    int n = fa ? fa->count : 0, err = 0;
    int saved[16], fds[16], ns = 0;
    for (int i = 0; i < n && !err; i++) {
        int seen = 0;
        for (int k = 0; k < ns; k++)
            seen |= fds[k] == v[i].fd;
        if (!seen && ns < 16) {
            fds[ns] = v[i].fd;
            saved[ns] = dup(v[i].fd);
            if (saved[ns] >= 0)
                fcntl(saved[ns], F_SETFD, FD_CLOEXEC);
            ns++;
        }
        if (v[i].kind == 2) {
            close(v[i].fd);
        } else if (v[i].kind == 1) {
            if (dup2(v[i].from, v[i].fd) < 0)
                err = errno;
        } else {
            int h = open(strcmp(v[i].path, "/dev/null") ? v[i].path : "NUL", v[i].flags & ~O_CLOEXEC, v[i].mode);
            if (h < 0) {
                err = errno;
            } else if (h != v[i].fd) {
                dup2(h, v[i].fd);
                close(h);
            }
        }
    }
    if (err) {
        free(env);
    }
    int r = err ? -1 : run_env(P_NOWAIT, find ? found : path, argv, env);
    if (!err && r < 0)
        err = errno;
    while (ns--) {
        if (saved[ns] >= 0) {
            dup2(saved[ns], fds[ns]);
            close(saved[ns]);
        } else {
            close(fds[ns]);
        }
    }
    _rs2_unlock(&lock);
    if (!err && pid)
        *pid = r;
    return err;
}

int posix_spawn(pid_t *__restrict pid, const char *__restrict path, const posix_spawn_file_actions_t *fa,
                const posix_spawnattr_t *__restrict at, char *const *__restrict argv, char *const *__restrict envp) {
    (void)at;
    return spawn(pid, path, fa, argv, envp, 0);
}
int posix_spawnp(pid_t *__restrict pid, const char *__restrict path, const posix_spawn_file_actions_t *fa,
                 const posix_spawnattr_t *__restrict at, char *const *__restrict argv, char *const *__restrict envp) {
    (void)at;
    return spawn(pid, path, fa, argv, envp, 1);
}

/* exec: the program runs and this one ends with its result. */
static int exec_(const char *path, char *const argv[], char *const envp[], int find) {
    pid_t pid;
    int e = spawn(&pid, path, 0, argv, envp, find), st = 0;
    if (e) {
        errno = e;
        return -1;
    }
    waitpid(pid, &st, 0);
    _exit(st & 0x7f ? 128 + (st & 0x7f) : (st >> 8) & 0xff);
}
int execv(const char *p, char *const a[]) { return exec_(p, a, 0, 0); }
int execve(const char *p, char *const a[], char *const e[]) { return exec_(p, a, e, 0); }
int execvp(const char *p, char *const a[]) { return exec_(p, a, 0, 1); }
pid_t fork(void) {
    errno = ENOSYS;
    return -1;
}

#include <signal.h>
int kill(pid_t pid, int sig) {
    if (sig == 0)
        return 0;
    APIRET rc = DosKillProcess(DKP_PROCESS, (PID)pid);
    if (rc) {
        errno = ESRCH;
        return -1;
    }
    return 0;
}

const char *getexecname(void) {
    static char name[264];
    if (!name[0])
        DosQueryModuleName(pib()->pib_hmte, sizeof name, (PCHAR)name);
    return name;
}
