#ifndef _TIME_H
#define _TIME_H
#include "_rs2libc.h"
_RS2_BEGIN
#ifndef _TIME_T_DEFINED
#define _TIME_T_DEFINED
typedef long time_t;
#endif
typedef long clock_t;
#define CLOCKS_PER_SEC 1000L
#define TIME_UTC 1
struct tm {
    int tm_sec, tm_min, tm_hour, tm_mday, tm_mon, tm_year, tm_wday, tm_yday, tm_isdst;
};
struct timespec {
    time_t tv_sec;
    long tv_nsec;
};
typedef int clockid_t;
#define CLOCK_REALTIME 0
#define CLOCK_MONOTONIC 1
int clock_gettime(clockid_t, struct timespec *);
int clock_getres(clockid_t, struct timespec *);
clock_t clock(void);
time_t time(time_t *);
double difftime(time_t, time_t);
time_t mktime(struct tm *);
int timespec_get(struct timespec *, int);
char *asctime(const struct tm *);
char *ctime(const time_t *);
struct tm *gmtime(const time_t *);
struct tm *localtime(const time_t *);
struct tm *gmtime_r(const time_t *__restrict, struct tm *__restrict);
struct tm *localtime_r(const time_t *__restrict, struct tm *__restrict);
size_t strftime(char *__restrict, size_t, const char *__restrict, const struct tm *__restrict);
int nanosleep(const struct timespec *, struct timespec *);
/* The local time zone, from the TZ variable ("EST5EDT", "CET-1"). */
void tzset(void);
extern long timezone _RS2_DATA(timezone);
extern int daylight _RS2_DATA(daylight);
extern char *tzname[2] _RS2_DATA(tzname);
_RS2_END
#endif
