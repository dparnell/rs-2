#ifndef _SYS_WAIT_H
#define _SYS_WAIT_H
#include <sys/types.h>
_RS2_BEGIN
#define WNOHANG 1
#define WUNTRACED 2
#define WIFEXITED(s) (((s) & 0x7f) == 0)
#define WEXITSTATUS(s) (((s) >> 8) & 0xff)
#define WIFSIGNALED(s) (((s) & 0x7f) != 0)
#define WTERMSIG(s) ((s) & 0x7f)
#define WIFSTOPPED(s) 0
#define WSTOPSIG(s) 0
#define WCOREDUMP(s) 0
struct rusage;
pid_t wait(int *);
pid_t waitpid(pid_t, int *, int);
pid_t wait4(pid_t, int *, int, struct rusage *);
_RS2_END
#endif
