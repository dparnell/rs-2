#ifndef _SYS_TIME_H
#define _SYS_TIME_H
#include <sys/types.h>
_RS2_BEGIN
struct timeval {
    time_t tv_sec;
    long tv_usec;
};
int utimes(const char *, const struct timeval[2]);
int gettimeofday(struct timeval *__restrict, void *__restrict);
_RS2_END
#endif
