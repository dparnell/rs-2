#ifndef _SIGNAL_H
#define _SIGNAL_H
#include "_rs2libc.h"
_RS2_BEGIN
typedef int sig_atomic_t;
typedef void (*__sighandler_t)(int);
#define SIG_DFL ((__sighandler_t)0)
#define SIG_IGN ((__sighandler_t)1)
#define SIG_ERR ((__sighandler_t)-1)
#define SIGINT 2
#define SIGILL 4
#define SIGABRT 6
#define SIGFPE 8
#define SIGKILL 9
#define SIGSEGV 11
#define SIGTERM 15
#define SIGBREAK 21
#define NSIG 32
__sighandler_t signal(int, __sighandler_t);
int raise(int);
_RS2_END
#endif
