#ifndef _SYS_RESOURCE_H
#define _SYS_RESOURCE_H
#include <sys/types.h>
#include <sys/time.h>
_RS2_BEGIN
typedef unsigned long rlim_t;
struct rlimit {
    rlim_t rlim_cur, rlim_max;
};
struct rusage {
    struct timeval ru_utime, ru_stime;
    long ru_maxrss, ru_minflt, ru_majflt, ru_nvcsw, ru_nivcsw;
};
#define RLIM_INFINITY (~0UL)
#define RLIMIT_CPU 0
#define RLIMIT_FSIZE 1
#define RLIMIT_DATA 2
#define RLIMIT_STACK 3
#define RLIMIT_CORE 4
#define RLIMIT_NOFILE 7
#define RLIMIT_AS 9
#define RUSAGE_SELF 0
#define RUSAGE_CHILDREN (-1)
int getrlimit(int, struct rlimit *);
int setrlimit(int, const struct rlimit *);
/* ru_utime: the time since the program's start. */
int getrusage(int, struct rusage *);
_RS2_END
#endif
