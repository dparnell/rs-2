#ifndef _PROCESS_H
#define _PROCESS_H
#include <sys/types.h>
_RS2_BEGIN
#define P_WAIT 0
#define P_NOWAIT 1
/* Threads that use the C library start with _beginthread. */
int _beginthread(void (*start)(void *), void *stack, unsigned stack_size, void *arg);
void _endthread(void);
int spawnv(int mode, const char *path, char *const argv[]);
int spawnvp(int mode, const char *file, char *const argv[]);
int spawnl(int mode, const char *path, const char *arg0, ...);
int spawnlp(int mode, const char *file, const char *arg0, ...);
int cwait(int *status, int pid, int action);
int wait(int *status);
pid_t getpid(void);
_RS2_END
#endif
