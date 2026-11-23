#ifndef _SIGNAL_H
#define _SIGNAL_H
#include <sys/types.h>
_RS2_BEGIN
typedef int sig_atomic_t;
typedef void (*__sighandler_t)(int);
#define SIG_DFL ((__sighandler_t)0)
#define SIG_IGN ((__sighandler_t)1)
#define SIG_ERR ((__sighandler_t)-1)
#define SIGHUP 1
#define SIGINT 2
#define SIGQUIT 3
#define SIGILL 4
#define SIGTRAP 5
#define SIGABRT 6
#define SIGBUS 7
#define SIGFPE 8
#define SIGKILL 9
#define SIGUSR1 10
#define SIGSEGV 11
#define SIGUSR2 12
#define SIGPIPE 13
#define SIGALRM 14
#define SIGTERM 15
#define SIGBREAK 21
#define SIGCHLD 17
#define SIGXCPU 24
#define SIGXFSZ 25
#define SIGSYS 31
#define NSIG 32
__sighandler_t signal(int, __sighandler_t);
int raise(int);
int kill(pid_t, int);
/* Of the signals above, OS/2 raises SIGINT, SIGBREAK, SIGTERM, SIGSEGV,
 * SIGILL and SIGFPE; none can be blocked. */
typedef unsigned long sigset_t;
union sigval {
    int sival_int;
    void *sival_ptr;
};
typedef struct {
    int si_signo, si_errno, si_code;
    pid_t si_pid;
    void *si_addr;
    union sigval si_value;
} siginfo_t;
struct sigaction {
    union {
        __sighandler_t sa_handler;
        void (*sa_sigaction)(int, siginfo_t *, void *);
    };
    sigset_t sa_mask;
    int sa_flags;
};
#define SA_NOCLDSTOP 1
#define SA_SIGINFO 4
#define SA_ONSTACK 0x08000000
#define SA_RESTART 0x10000000
#define SA_NODEFER 0x40000000
#define SA_RESETHAND 0x80000000
#define SIG_BLOCK 0
#define SIG_UNBLOCK 1
#define SIG_SETMASK 2
int sigaction(int, const struct sigaction *__restrict, struct sigaction *__restrict);
int sigemptyset(sigset_t *);
int sigfillset(sigset_t *);
int sigaddset(sigset_t *, int);
int sigdelset(sigset_t *, int);
int sigismember(const sigset_t *, int);
int sigprocmask(int, const sigset_t *__restrict, sigset_t *__restrict);
int pthread_sigmask(int, const sigset_t *__restrict, sigset_t *__restrict);
_RS2_END
#endif
