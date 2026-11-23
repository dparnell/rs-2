/* <time.h>. The clock runs on local time; TZ ("EST5EDT", "CET-1CEST",
 * with POSIX ",M3.2.0,M11.1.0" or OS/2 ",3,2,0,7200,11,1,0,7200,3600"
 * rules) says how far that is from UTC. Without TZ, local time is UTC. */
#include "libc.h"
#include <time.h>
#include <string.h>
#include <stdlib.h>
#include <stdio.h>
#include <ctype.h>
#include <sys/time.h>
#include <unistd.h>

ULONG _rs2_start_ms;
long timezone;
int daylight;
static char std_name[16] = "UTC", dst_name[16] = "";
char *tzname[2] = {std_name, dst_name};

long long _rs2_days_from_civil(long long y, unsigned m, unsigned d) {
    y -= m <= 2;
    long long era = (y >= 0 ? y : y - 399) / 400;
    unsigned yoe = (unsigned)(y - era * 400);
    unsigned doy = (153 * (m + (m > 2 ? -3 : 9)) + 2) / 5 + d - 1;
    unsigned doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    return era * 146097 + (long long)doe - 719468;
}

static void civil_from_days(long long z, int *y, int *m, int *d) {
    z += 719468;
    long long era = (z >= 0 ? z : z - 146096) / 146097;
    unsigned doe = (unsigned)(z - era * 146097);
    unsigned yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    long long yy = (long long)yoe + era * 400;
    unsigned doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    unsigned mp = (5 * doy + 2) / 153;
    *d = (int)(doy - (153 * mp + 2) / 5 + 1);
    *m = (int)(mp < 10 ? mp + 3 : mp - 9);
    *y = (int)(yy + (*m <= 2));
}

static int is_leap(long long y) {
    return (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
}

static int days_in_month(long long y, int m) {
    static const char dm[] = {31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31};
    return m == 2 && is_leap(y) ? 29 : dm[m - 1];
}

/* ---- time zones ---- */

typedef struct {
    int month, week, wday, mday;   /* week 5 or -1: the last */
    long secs;                      /* local time of day */
} rule;

static long std_off, dst_off;       /* UTC = local + off */
static int has_dst;
static rule dst_start = {3, 2, 0, 0, 7200}, dst_end = {11, 1, 0, 0, 7200};
static char tz_seen[64];
static int tz_ready;

static const char *parse_name(const char *p, char *out) {
    int n = 0;
    if (*p == '<') {
        p++;
        while (*p && *p != '>')
            if (n < 15)
                out[n++] = *p++;
            else
                p++;
        if (*p)
            p++;
    } else {
        while (isalpha((unsigned char)*p))
            if (n < 15)
                out[n++] = *p++;
            else
                p++;
    }
    out[n] = 0;
    return p;
}

static const char *parse_offset(const char *p, long *out) {
    int neg = 0;
    if (*p == '+' || *p == '-')
        neg = *p++ == '-';
    long h = 0, m = 0, s = 0;
    while (isdigit((unsigned char)*p))
        h = h * 10 + (*p++ - '0');
    if (*p == ':') {
        p++;
        while (isdigit((unsigned char)*p))
            m = m * 10 + (*p++ - '0');
        if (*p == ':') {
            p++;
            while (isdigit((unsigned char)*p))
                s = s * 10 + (*p++ - '0');
        }
    }
    *out = (h * 3600 + m * 60 + s) * (neg ? -1 : 1);
    return p;
}

static const char *parse_rule(const char *p, rule *r) {
    if (*p == 'M') {
        r->month = (int)strtol(p + 1, (char **)&p, 10);
        if (*p == '.')
            r->week = (int)strtol(p + 1, (char **)&p, 10);
        if (*p == '.')
            r->wday = (int)strtol(p + 1, (char **)&p, 10);
        r->mday = 0;
        r->secs = 7200;
        if (*p == '/')
            p = parse_offset(p + 1, &r->secs);
    }
    return p;
}

void tzset(void) {
    const char *tz = getenv("TZ");
    if (!tz)
        tz = "";
    if (tz_ready && !strncmp(tz, tz_seen, sizeof tz_seen - 1))
        return;
    strncpy(tz_seen, tz, sizeof tz_seen - 1);
    tz_ready = 1;
    std_off = dst_off = 0;
    has_dst = 0;
    strcpy(std_name, "UTC");
    dst_name[0] = 0;
    const char *p = tz;
    if (*p == ':')
        p++;
    if (*p) {
        p = parse_name(p, std_name);
        p = parse_offset(p, &std_off);
        if (isalpha((unsigned char)*p) || *p == '<') {
            p = parse_name(p, dst_name);
            has_dst = 1;
            dst_off = std_off - 3600;
            if (*p && *p != ',')
                p = parse_offset(p, &dst_off);
            if (*p == ',') {
                p++;
                if (*p == 'M') {
                    p = parse_rule(p, &dst_start);
                    if (*p == ',')
                        parse_rule(p + 1, &dst_end);
                } else {
                    /* OS/2: start month, week, day, seconds; the same for
                     * the end; then the shift in seconds. */
                    long v[9] = {3, 2, 0, 7200, 11, 1, 0, 7200, 3600};
                    for (int i = 0; i < 9 && *p; i++) {
                        v[i] = strtol(p, (char **)&p, 10);
                        if (*p == ',')
                            p++;
                    }
                    dst_start = (rule){(int)v[0], (int)v[1], (int)v[2], (int)v[1] == 0 ? (int)v[2] : 0, v[3]};
                    dst_end = (rule){(int)v[4], (int)v[5], (int)v[6], (int)v[5] == 0 ? (int)v[6] : 0, v[7]};
                    dst_off = std_off - v[8];
                }
            }
        }
    }
    timezone = std_off;
    daylight = has_dst;
}

/* The local seconds (in the year's own time) when a rule fires. */
static long long rule_time(long long year, const rule *r) {
    int day;
    if (r->mday) {
        day = r->mday;
    } else {
        long long first = _rs2_days_from_civil(year, r->month, 1);
        int wd1 = (int)((first % 7 + 11) % 7);   /* 1970-01-01 was a Thursday */
        day = 1 + (r->wday - wd1 + 7) % 7;
        int w = r->week < 0 ? 5 : r->week;
        day += (w - 1) * 7;
        while (day > days_in_month(year, r->month))
            day -= 7;
    }
    return _rs2_days_from_civil(year, r->month, day) * 86400 + r->secs;
}

long _rs2_tz_offset(time_t utc, int *isdst) {
    tzset();
    *isdst = 0;
    if (!has_dst)
        return std_off;
    int y, m, d;
    civil_from_days((utc - std_off) / 86400 - ((utc - std_off) % 86400 < 0), &y, &m, &d);
    long long start = rule_time(y, &dst_start) + std_off, end = rule_time(y, &dst_end) + dst_off;
    int in = start < end ? utc >= start && utc < end : utc >= start || utc < end;
    *isdst = in;
    return in ? dst_off : std_off;
}

/* ---- the clock ---- */

static time_t now(int *hundredths) {
    DATETIME dt;
    DosGetDateTime(&dt);
    if (hundredths)
        *hundredths = dt.hundredths;
    time_t local = _rs2_days_from_civil(dt.year, dt.month, dt.day) * 86400 + dt.hours * 3600 + dt.minutes * 60 + dt.seconds;
    int dst;
    if (!getenv("TZ") && dt.timezone != -1)
        return local + dt.timezone * 60;
    /* The offset at that moment: try standard time, then check. */
    tzset();
    time_t t = local + std_off;
    return local + _rs2_tz_offset(t, &dst);
}

time_t time(time_t *t) {
    time_t r = now(0);
    if (t)
        *t = r;
    return r;
}

int gettimeofday(struct timeval *__restrict tv, void *__restrict tz) {
    (void)tz;
    int h;
    tv->tv_sec = now(&h);
    tv->tv_usec = h * 10000L;
    return 0;
}

/* The wall clock has 10 ms steps: in between, the fine clock carries it
 * on from where the two last met. They meet again when they are apart by
 * more than 20 ms (the clock was set, or drifted). */
int timespec_get(struct timespec *ts, int base) {
    if (base != TIME_UTC)
        return 0;
    static volatile int lock;
    static long long offset;   /* wall - fine, in nanoseconds */
    static int anchored;
    int h;
    struct timespec m;
    _rs2_spin_lock(&lock);
    long long wall = now(&h) * 1000000000LL;
    wall += h * 10000000LL;
    clock_gettime(CLOCK_MONOTONIC, &m);
    long long fine = m.tv_sec * 1000000000LL + m.tv_nsec, t = fine + offset;
    if (!anchored || t < wall - 20000000 || t > wall + 30000000) {
        offset = wall - fine;
        anchored = 1;
        t = wall;
    }
    _rs2_spin_unlock(&lock);
    ts->tv_sec = (time_t)(t / 1000000000);
    ts->tv_nsec = (long)(t % 1000000000);
    return base;
}

clock_t clock(void) {
    ULONG ms;
    DosQuerySysInfo(QSV_MS_COUNT, QSV_MS_COUNT, &ms, sizeof ms);
    if (!_rs2_start_ms)
        _rs2_start_ms = ms;
    return (clock_t)(ULONG)(ms - _rs2_start_ms);
}

double difftime(time_t a, time_t b) {
    return (double)(a - b);
}

unsigned sleep(unsigned s) {
    DosSleep(s * 1000);
    return 0;
}

int usleep(unsigned long us) {
    DosSleep((ULONG)((us + 999) / 1000));
    return 0;
}

int nanosleep(const struct timespec *req, struct timespec *rem) {
    DosSleep((ULONG)(req->tv_sec * 1000 + (req->tv_nsec + 999999) / 1000000));
    if (rem)
        rem->tv_sec = rem->tv_nsec = 0;
    return 0;
}

/* ---- broken-down time ---- */

struct tm *gmtime_r(const time_t *__restrict t, struct tm *__restrict tm) {
    time_t v = *t;
    long long days = v / 86400;
    long secs = (long)(v % 86400);
    if (secs < 0)
        secs += 86400, days--;
    int y, m, d;
    civil_from_days(days, &y, &m, &d);
    tm->tm_year = y - 1900;
    tm->tm_mon = m - 1;
    tm->tm_mday = d;
    tm->tm_hour = (int)(secs / 3600);
    tm->tm_min = (int)(secs / 60 % 60);
    tm->tm_sec = (int)(secs % 60);
    tm->tm_wday = (int)((days % 7 + 11) % 7);
    tm->tm_yday = (int)(days - _rs2_days_from_civil(y, 1, 1));
    tm->tm_isdst = 0;
    return tm;
}

struct tm *localtime_r(const time_t *__restrict t, struct tm *__restrict tm) {
    int dst;
    long off = _rs2_tz_offset(*t, &dst);
    time_t local = *t - off;
    gmtime_r(&local, tm);
    tm->tm_isdst = dst;
    return tm;
}

struct tm *gmtime(const time_t *t) {
    return gmtime_r(t, &_rs2_self()->tm);
}

struct tm *localtime(const time_t *t) {
    return localtime_r(t, &_rs2_self()->tm);
}

time_t mktime(struct tm *tm) {
    /* Normalise month and year, then count the rest as seconds. */
    long long y = tm->tm_year + 1900LL, mon = tm->tm_mon;
    y += mon / 12;
    mon %= 12;
    if (mon < 0)
        mon += 12, y--;
    time_t local = (_rs2_days_from_civil(y, (unsigned)mon + 1, 1) + tm->tm_mday - 1) * 86400LL + tm->tm_hour * 3600LL +
                   tm->tm_min * 60LL + tm->tm_sec;
    tzset();
    int dst;
    time_t t;
    if (tm->tm_isdst > 0 && has_dst)
        t = local + dst_off;
    else if (tm->tm_isdst == 0)
        t = local + std_off;
    else {
        t = local + std_off;
        long off = _rs2_tz_offset(t, &dst);
        t = local + off;
    }
    localtime_r(&t, tm);
    return t;
}

static const char *const wdays[] = {"Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"};
static const char *const months[] = {"January", "February", "March", "April", "May", "June",
                                     "July", "August", "September", "October", "November", "December"};

char *asctime(const struct tm *tm) {
    char *b = _rs2_self()->asctime;
    snprintf(b, 32, "%.3s %.3s%3d %.2d:%.2d:%.2d %d\n", wdays[tm->tm_wday % 7], months[tm->tm_mon % 12], tm->tm_mday,
             tm->tm_hour, tm->tm_min, tm->tm_sec, 1900 + tm->tm_year);
    return b;
}

char *ctime(const time_t *t) {
    return asctime(localtime(t));
}

/* ISO 8601 week-based year and week. */
static int iso_week(const struct tm *tm, int *year) {
    int y = tm->tm_year + 1900;
    int wday = (tm->tm_wday + 6) % 7;   /* Monday = 0 */
    int week = (tm->tm_yday - wday + 10) / 7;
    if (week < 1) {
        y--;
        int ydays = is_leap(y) ? 366 : 365;
        week = (tm->tm_yday + ydays - wday + 10) / 7;
    } else if (week == 53) {
        int ydays = is_leap(y) ? 366 : 365;
        if (tm->tm_yday - wday + 3 >= ydays)
            week = 1, y++;
    }
    *year = y;
    return week;
}

size_t strftime(char *__restrict s, size_t max, const char *__restrict fmt, const struct tm *__restrict tm) {
    size_t n = 0;
    char tmp[64];
#define ADD(str) do { const char *q_ = (str); while (*q_) { if (n + 1 >= max) return 0; s[n++] = *q_++; } } while (0)
    for (; *fmt; fmt++) {
        if (*fmt != '%') {
            if (n + 1 >= max)
                return 0;
            s[n++] = *fmt;
            continue;
        }
        fmt++;
        if (*fmt == 'E' || *fmt == 'O')
            fmt++;
        int y;
        switch (*fmt) {
        case 'a': snprintf(tmp, sizeof tmp, "%.3s", wdays[tm->tm_wday % 7]); break;
        case 'A': snprintf(tmp, sizeof tmp, "%s", wdays[tm->tm_wday % 7]); break;
        case 'b': case 'h': snprintf(tmp, sizeof tmp, "%.3s", months[tm->tm_mon % 12]); break;
        case 'B': snprintf(tmp, sizeof tmp, "%s", months[tm->tm_mon % 12]); break;
        case 'c':
            snprintf(tmp, sizeof tmp, "%.3s %.3s %2d %.2d:%.2d:%.2d %d", wdays[tm->tm_wday % 7], months[tm->tm_mon % 12],
                     tm->tm_mday, tm->tm_hour, tm->tm_min, tm->tm_sec, 1900 + tm->tm_year);
            break;
        case 'C': snprintf(tmp, sizeof tmp, "%02d", (1900 + tm->tm_year) / 100); break;
        case 'd': snprintf(tmp, sizeof tmp, "%02d", tm->tm_mday); break;
        case 'D': snprintf(tmp, sizeof tmp, "%02d/%02d/%02d", tm->tm_mon + 1, tm->tm_mday, tm->tm_year % 100); break;
        case 'e': snprintf(tmp, sizeof tmp, "%2d", tm->tm_mday); break;
        case 'F': snprintf(tmp, sizeof tmp, "%d-%02d-%02d", 1900 + tm->tm_year, tm->tm_mon + 1, tm->tm_mday); break;
        case 'g': iso_week(tm, &y); snprintf(tmp, sizeof tmp, "%02d", y % 100); break;
        case 'G': iso_week(tm, &y); snprintf(tmp, sizeof tmp, "%d", y); break;
        case 'H': snprintf(tmp, sizeof tmp, "%02d", tm->tm_hour); break;
        case 'I': snprintf(tmp, sizeof tmp, "%02d", tm->tm_hour % 12 ? tm->tm_hour % 12 : 12); break;
        case 'j': snprintf(tmp, sizeof tmp, "%03d", tm->tm_yday + 1); break;
        case 'm': snprintf(tmp, sizeof tmp, "%02d", tm->tm_mon + 1); break;
        case 'M': snprintf(tmp, sizeof tmp, "%02d", tm->tm_min); break;
        case 'n': strcpy(tmp, "\n"); break;
        case 'p': strcpy(tmp, tm->tm_hour < 12 ? "AM" : "PM"); break;
        case 'r':
            snprintf(tmp, sizeof tmp, "%02d:%02d:%02d %s", tm->tm_hour % 12 ? tm->tm_hour % 12 : 12, tm->tm_min, tm->tm_sec,
                     tm->tm_hour < 12 ? "AM" : "PM");
            break;
        case 'R': snprintf(tmp, sizeof tmp, "%02d:%02d", tm->tm_hour, tm->tm_min); break;
        case 's': {
            struct tm c = *tm;
            snprintf(tmp, sizeof tmp, "%ld", (long)mktime(&c));
            break;
        }
        case 'S': snprintf(tmp, sizeof tmp, "%02d", tm->tm_sec); break;
        case 't': strcpy(tmp, "\t"); break;
        case 'T': snprintf(tmp, sizeof tmp, "%02d:%02d:%02d", tm->tm_hour, tm->tm_min, tm->tm_sec); break;
        case 'u': snprintf(tmp, sizeof tmp, "%d", tm->tm_wday ? tm->tm_wday : 7); break;
        case 'U': snprintf(tmp, sizeof tmp, "%02d", (tm->tm_yday + 7 - tm->tm_wday) / 7); break;
        case 'V': snprintf(tmp, sizeof tmp, "%02d", iso_week(tm, &y)); break;
        case 'w': snprintf(tmp, sizeof tmp, "%d", tm->tm_wday); break;
        case 'W': snprintf(tmp, sizeof tmp, "%02d", (tm->tm_yday + 7 - (tm->tm_wday + 6) % 7) / 7); break;
        case 'x': snprintf(tmp, sizeof tmp, "%02d/%02d/%02d", tm->tm_mon + 1, tm->tm_mday, tm->tm_year % 100); break;
        case 'X': snprintf(tmp, sizeof tmp, "%02d:%02d:%02d", tm->tm_hour, tm->tm_min, tm->tm_sec); break;
        case 'y': snprintf(tmp, sizeof tmp, "%02d", tm->tm_year % 100); break;
        case 'Y': snprintf(tmp, sizeof tmp, "%d", 1900 + tm->tm_year); break;
        case 'z': {
            tzset();
            long off = tm->tm_isdst > 0 ? dst_off : std_off;
            long a = off < 0 ? -off : off;
            snprintf(tmp, sizeof tmp, "%c%02ld%02ld", off <= 0 ? '+' : '-', a / 3600, a / 60 % 60);
            break;
        }
        case 'Z': tzset(); snprintf(tmp, sizeof tmp, "%s", tm->tm_isdst > 0 ? dst_name : std_name); break;
        case '%': strcpy(tmp, "%"); break;
        case 0: fmt--; tmp[0] = 0; break;
        default: snprintf(tmp, sizeof tmp, "%%%c", *fmt); break;
        }
        ADD(tmp);
    }
    s[n] = 0;
    return n;
#undef ADD
}
