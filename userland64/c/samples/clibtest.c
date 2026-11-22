/* CLIBTEST.EXE: tests rs2's 64-bit C library (docs/LIBC64.md) on rs2.
 * Prints one line per check and "clibtest: N failures"; exits with N.
 * "CLIBTEST child ARGS" is the child the spawn test starts. */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <errno.h>
#include <limits.h>
#include <float.h>
#include <math.h>
#include <time.h>
#include <setjmp.h>
#include <signal.h>
#include <assert.h>
#include <stdint.h>
#include <inttypes.h>
#include <fcntl.h>
#include <unistd.h>
#include <io.h>
#include <direct.h>
#include <dirent.h>
#include <process.h>
#include <sys/stat.h>
#include <os2.h>

static int failures, checks;

static void check(const char *what, int ok) {
    checks++;
    printf("  %s: %s\n", what, ok ? "ok" : "FAILED");
    if (!ok)
        failures++;
}

static void check_str(const char *what, const char *got, const char *want) {
    checks++;
    if (strcmp(got, want)) {
        printf("  %s: FAILED: got \"%s\", want \"%s\"\n", what, got, want);
        failures++;
    } else {
        printf("  %s: ok\n", what);
    }
}

static int close_to(double a, double b, double tol) {
    if (a == b)
        return 1;
    return fabs(a - b) <= tol * fabs(b);
}

static void check_num(const char *what, double got, double want, double tol) {
    checks++;
    if (!close_to(got, want, tol)) {
        printf("  %s: FAILED: got %.17g, want %.17g\n", what, got, want);
        failures++;
    } else {
        printf("  %s: ok\n", what);
    }
}

/* ---- strings, ctype, conversions ---- */

static void test_strings(void) {
    char b[64];
    check("strlen", strlen("hello") == 5 && strlen("") == 0);
    check("strcmp", strcmp("abc", "abd") < 0 && strcmp("b", "a") > 0 && strcmp("x", "x") == 0);
    check("strncmp", strncmp("abcdef", "abcxyz", 3) == 0 && strncmp("ab", "abc", 3) < 0);
    check("strcasecmp", strcasecmp("HeLLo", "hello") == 0 && stricmp("A", "b") < 0);
    strcpy(b, "hello");
    strcat(b, ", world");
    check_str("strcpy/strcat", b, "hello, world");
    check("strchr/strrchr", strchr(b, 'o') == b + 4 && strrchr(b, 'o') == b + 8 && !strchr(b, 'z'));
    check("strstr", strstr(b, "world") == b + 7 && strstr(b, "") == b && !strstr(b, "worlds"));
    check("strspn/strcspn", strspn("aabbc", "ab") == 4 && strcspn("hello", "lo") == 2);
    strcpy(b, "a,b,,c");
    char *t1 = strtok(b, ","), *t2 = strtok(0, ","), *t3 = strtok(0, ","), *t4 = strtok(0, ",");
    check("strtok", t1 && !strcmp(t1, "a") && t2 && !strcmp(t2, "b") && t3 && !strcmp(t3, "c") && !t4);
    strcpy(b, "0123456789");
    memmove(b + 2, b, 5);
    check_str("memmove up", b, "0101234789");
    strcpy(b, "0123456789");
    memmove(b, b + 2, 5);
    check_str("memmove down", b, "2345656789");
    memset(b, 'x', 3);
    b[3] = 0;
    check_str("memset", b, "xxx");
    char *d = strdup("copy");
    check("strdup", d && !strcmp(d, "copy"));
    free(d);
    check("strerror", !strcmp(strerror(ENOENT), "No such file or directory"));
    check("ctype", isalpha('a') && !isalpha('1') && isdigit('7') && isspace('\t') && toupper('q') == 'Q' &&
                       tolower('Q') == 'q' && isxdigit('F') && !isxdigit('g') && ispunct('!'));
    char *end;
    check("strtol", strtol("  -1234xyz", &end, 10) == -1234 && *end == 'x');
    check("strtol bases", strtol("0x1f", 0, 0) == 31 && strtol("017", 0, 0) == 15 && strtol("z", 0, 36) == 35);
    errno = 0;
    long big = strtol("99999999999999999999", 0, 10);
    check("strtol overflow", big == LONG_MAX && errno == ERANGE);
    check("strtoul", strtoul("18446744073709551615", 0, 10) == ULONG_MAX);
    check("atoi", atoi("42") == 42 && atoi("-7") == -7);
    char ib[32];
    check_str("itoa", itoa(-255, ib, 10), "-255");
    check_str("ultoa hex", ultoa(0xdeadbeef, ib, 16), "deadbeef");
    div_t q = div(17, 5);
    check("div", q.quot == 3 && q.rem == 2 && abs(-3) == 3 && llabs(-5LL) == 5);
}

/* ---- printf ---- */

static void fmt(const char *what, const char *want, const char *f, ...) {
    char b[1024];
    va_list ap;
    va_start(ap, f);
    int n = vsnprintf(b, sizeof b, f, ap);
    va_end(ap);
    checks++;
    if (strcmp(b, want) || n != (int)strlen(want)) {
        printf("  printf %s: FAILED: got \"%s\" (%d), want \"%s\"\n", what, b, n, want);
        failures++;
    } else {
        printf("  printf %s: ok\n", what);
    }
}

static void test_printf(void) {
    fmt("ints", "42|   42|42   |00042|-42|+42| 42", "%d|%5d|%-5d|%05d|%d|%+d|% d", 42, 42, 42, 42, -42, 42, 42);
    fmt("hex", "ff FF 0xff 0XFF 17 017 0", "%x %X %#x %#X %o %#o %#x", 255, 255, 255, 255, 15, 15, 0);
    fmt("precision", "00042 -0042 ", "%.5d %.4d %.0d", 42, -42, 0);
    fmt("strings", "hi|hel|        hi|hi        |", "%s|%.3s|%10s|%-10s|", "hi", "hello", "hi", "hi");
    fmt("char", "[x] %", "[%c] %%", 'x');
    fmt("long", "-9223372036854775808 18446744073709551615", "%ld %llu", LONG_MIN, ULLONG_MAX);
    fmt("sizes", "255 65535 -1 7", "%hhu %hu %zd %jd", 511, 131071, (ssize_t)-1, (intmax_t)7);
    fmt("pointer", "0x0 0x1234", "%p %p", (void *)0, (void *)0x1234);
    fmt("star", "   ab|3.14", "%*s|%.*f", 5, "ab", 2, 3.14159);
    fmt("%f", "3.141590 2.67 3.1416", "%f %.2f %.4f", 3.14159, 2.675, 3.14159265);
    /* Exact ties go to even; 0.135 is just above its tie. */
    fmt("half even", "0 2 2 4 0.12 0.38 0.14", "%.0f %.0f %.0f %.0f %.2f %.2f %.2f", 0.5, 1.5, 2.5, 3.5, 0.125, 0.375, 0.135);
    fmt("%e", "1.234568e+04 0.000e+00 1e+100 -1.5E-07", "%e %.3e %.0e %.1E", 12345.678, 0.0, 1e100, -1.5e-7);
    fmt("%g", "0.0001 1e-05 1.23457e+08 100000 1e+06 0.5 1", "%g %g %g %g %g %g %g", 0.0001, 0.00001, 123456789.0, 100000.0,
        1000000.0, 0.5, 1.0);
    fmt("%#g", "1.00000 0.000100000", "%#g %#g", 1.0, 0.0001);
    fmt("%.17g", "0.10000000000000001 3.1415926535897931", "%.17g %.17g", 0.1, M_PI);
    fmt("width", "   -3.1416|+1.0e+01|0003.50", "%10.4f|%+.1e|%07.2f", -3.14159, 9.96, 3.5);
    fmt("inf nan", "inf -inf nan INF", "%f %f %f %F", INFINITY, -INFINITY, NAN, INFINITY);
    fmt("%a", "0x1p+0 0x1.8p+1 -0x1.999999999999ap-4 0x1.0p+0", "%a %a %a %.1a", 1.0, 3.0, -0.1, 1.0);
    fmt("DBL_MAX",
        "17976931348623157081452742373170435679807056752584499659891747680315726078002853876058955863276687817154045895"
        "35143824642343213268894641827684675467035375169860499105765512820762454900903893289440758685084551339423045832"
        "3690322294816580855933212334827479782620414472316873817718091929988125040402618412485836" "8",
        "%.0f", DBL_MAX);
    fmt("denormal", "4.94065645841246544177e-324", "%.20e", 4.9406564584124654e-324);
    fmt("small %f", "0.000000 0.000001", "%f %f", 1e-7, 6e-7);
    int n = 0;
    char b[32];
    snprintf(b, sizeof b, "abc%nxyz", &n);
    check("printf %n", n == 3);
    check("snprintf truncates", snprintf(b, 5, "%s", "hello world") == 11 && !strcmp(b, "hell"));
    char *s = 0;
    check("asprintf", asprintf(&s, "%d-%s", 7, "x") == 3 && s && !strcmp(s, "7-x"));
    free(s);
}

/* ---- scanf, strtod ---- */

static void test_scanf(void) {
    int i = 0, x = 0, n = 0;
    char w[32] = "", lw[32] = "", c = 0;
    double f = 0;
    int r = sscanf("  -17 word 2.5e1 1F abcXYZ!", "%d %31s %lf %x %[a-z]%n%c", &i, w, &f, &x, lw, &n, &c);
    check("sscanf", r == 6 && i == -17 && !strcmp(w, "word") && f == 25.0 && x == 0x1f && !strcmp(lw, "abc") && n == 23 &&
                        c == 'X');
    long long ll;
    unsigned u;
    float fl;
    r = sscanf("123456789012 0x10 0.5", "%lld %i %f", &ll, &u, &fl);
    check("sscanf sizes", r == 3 && ll == 123456789012LL && u == 16 && fl == 0.5f);
    check("sscanf eof", sscanf("", "%d", &i) == EOF && sscanf("abc", "%d", &i) == 0);
    char *end;
    check("strtod", strtod("3.14159", &end) == 3.14159 && *end == 0);
    check("strtod exp", strtod("1e-300", 0) == 1e-300 && strtod("-2.5E+3", 0) == -2500.0);
    check("strtod hex", strtod("0x1.8p1", 0) == 3.0 && strtod("0x10", 0) == 16.0);
    check("strtod inf nan", isinf(strtod("inf", 0)) && isnan(strtod("nan", 0)) && strtod("-Infinity", 0) < 0);
    errno = 0;
    check("strtod range", isinf(strtod("1e400", 0)) && errno == ERANGE);
    check("strtod denormal", strtod("4.9406564584124654e-324", 0) == 4.9406564584124654e-324);
    check("strtod max", strtod("1.7976931348623157e308", 0) == DBL_MAX);
    /* Round trips: printf %.17g, then strtod. */
    unsigned long long seed = 12345;
    int bad = 0;
    for (int k = 0; k < 20000; k++) {
        seed = seed * 6364136223846793005ULL + 1442695040888963407ULL;
        union { double d; unsigned long long u; } v;
        v.u = seed;
        if (!isfinite(v.d))
            continue;
        char b[64];
        snprintf(b, sizeof b, "%.17g", v.d);
        double back = strtod(b, 0);
        if (back != v.d) {
            if (bad < 3)
                printf("    round trip: %s -> %.17g\n", b, back);
            bad++;
        }
    }
    printf("    %d of 20000 round trips differ\n", bad);
    check("strtod round trips", bad == 0);
}

/* ---- memory ---- */

static void test_malloc(void) {
    enum { N = 2000 };
    static char *p[N];
    static size_t sz[N];
    unsigned seed = 1;
    int ok = 1;
    for (int round = 0; round < 20; round++) {
        for (int i = 0; i < N; i++) {
            if (p[i] && rand_r(&seed) % 3 == 0) {
                for (size_t k = 0; k < sz[i]; k++)
                    if (p[i][k] != (char)(i + k))
                        ok = 0;
                free(p[i]);
                p[i] = 0;
            }
            if (!p[i]) {
                sz[i] = rand_r(&seed) % (rand_r(&seed) % 10 == 0 ? 70000 : 300);
                p[i] = malloc(sz[i]);
                if (!p[i]) {
                    ok = 0;
                    continue;
                }
                for (size_t k = 0; k < sz[i]; k++)
                    p[i][k] = (char)(i + k);
            } else if (rand_r(&seed) % 5 == 0) {
                size_t ns = rand_r(&seed) % 500;
                char *q = realloc(p[i], ns);
                if (!q && ns) {
                    ok = 0;
                    continue;
                }
                size_t keep = ns < sz[i] ? ns : sz[i];
                for (size_t k = 0; k < keep; k++)
                    if (q[k] != (char)(i + k))
                        ok = 0;
                for (size_t k = keep; k < ns; k++)
                    q[k] = (char)(i + k);
                p[i] = q;
                sz[i] = ns;
            }
        }
    }
    for (int i = 0; i < N; i++)
        free(p[i]);
    check("malloc/realloc/free", ok);
    char *big = malloc(8 << 20);
    check("malloc 8 MB", big != 0);
    if (big) {
        memset(big, 1, 8 << 20);
        big = realloc(big, 16 << 20);
        check("realloc 16 MB", big && big[(8 << 20) - 1] == 1);
        free(big);
    }
    int *z = calloc(1000, sizeof(int));
    int zero = 1;
    for (int i = 0; z && i < 1000; i++)
        zero &= z[i] == 0;
    check("calloc", z && zero);
    free(z);
    void *a = aligned_alloc(4096, 100);
    check("aligned_alloc", a && ((uintptr_t)a & 4095) == 0);
    free(a);
    check("malloc below 4 GB", (uintptr_t)malloc(10) < 0x100000000ULL);
}

/* ---- sorting ---- */

static int cmp_int(const void *a, const void *b) {
    int x = *(const int *)a, y = *(const int *)b;
    return x < y ? -1 : x > y;
}

static void test_sort(void) {
    enum { N = 5000 };
    static int v[N];
    srand(7);
    for (int i = 0; i < N; i++)
        v[i] = rand() % 1000;
    qsort(v, N, sizeof v[0], cmp_int);
    int ok = 1;
    for (int i = 1; i < N; i++)
        ok &= v[i - 1] <= v[i];
    check("qsort", ok);
    int key = v[N / 2];
    int *f = bsearch(&key, v, N, sizeof v[0], cmp_int);
    check("bsearch", f && *f == key);
    key = 5000;
    check("bsearch missing", !bsearch(&key, v, N, sizeof v[0], cmp_int));
}

/* ---- math ---- */

static void test_math(void) {
    check_num("sqrt", sqrt(2), 1.4142135623730951, 0);
    check_num("sin", sin(M_PI / 6), 0.5, 1e-15);
    check_num("cos", cos(M_PI / 3), 0.5, 1e-15);
    check_num("tan", tan(M_PI / 4), 1, 1e-15);
    check_num("sin big", sin(1e22), -0.8522008497671888, 1e-6);
    check_num("exp", exp(1), M_E, 1e-15);
    check_num("log", log(M_E), 1, 1e-15);
    check_num("log10", log10(1000), 3, 1e-15);
    check_num("log2", log2(1024), 10, 0);
    check_num("pow", pow(2, 10), 1024, 0);
    check_num("pow frac", pow(2, 0.5), M_SQRT2, 1e-15);
    check_num("pow neg", pow(-2, 3), -8, 0);
    check_num("atan2", atan2(1, 1), M_PI_4, 1e-15);
    check_num("atan2 quadrant", atan2(-1, -1), -3 * M_PI_4, 1e-15);
    check_num("asin", asin(0.5), M_PI / 6, 1e-15);
    check_num("acos", acos(0.5), M_PI / 3, 1e-15);
    check_num("sinh", sinh(1), 1.1752011936438014, 1e-15);
    check_num("tanh", tanh(0.5), 0.46211715726000974, 1e-15);
    check_num("hypot", hypot(3, 4), 5, 0);
    check_num("cbrt", cbrt(27), 3, 1e-15);
    check_num("expm1", expm1(1e-10), 1.00000000005e-10, 1e-15);
    check_num("log1p", log1p(1e-10), 9.9999999995e-11, 1e-15);
    check_num("erf", erf(1), 0.8427007929497149, 1e-15);
    check_num("erfc", erfc(3), 2.209049699858544e-05, 1e-13);
    check_num("tgamma", tgamma(5), 24, 1e-14);
    check_num("lgamma", lgamma(0.5), 0.5723649429247001, 1e-14);
    check("floor/ceil", floor(-1.5) == -2 && ceil(-1.5) == -1 && floor(2.0) == 2 && ceil(2.1) == 3);
    check("round/trunc", round(-2.5) == -3 && round(2.4) == 2 && trunc(-2.7) == -2 && rint(2.5) == 2);
    check("fmod", fmod(7.5, 2) == 1.5 && fmod(-7.5, 2) == -1.5);
    int e;
    double m = frexp(48, &e);
    check("frexp/ldexp", m == 0.75 && e == 6 && ldexp(0.75, 6) == 48);
    double ip;
    check("modf", modf(3.25, &ip) == 0.25 && ip == 3);
    errno = 0;
    check("domain error", isnan(sqrt(-1)) && errno == EDOM);
    check("classify", isnan(NAN) && isinf(INFINITY) && fpclassify(0.0) == FP_ZERO && signbit(-0.0));
}

/* ---- setjmp, signals ---- */

static jmp_buf jb;
static volatile int sig_seen;

static void deep(int n) {
    if (n == 0)
        longjmp(jb, 42);
    deep(n - 1);
}

static void on_signal(int sig) {
    sig_seen = sig;
    longjmp(jb, sig);
}

static void on_term(int sig) {
    sig_seen = sig;
}

static void test_signals(void) {
    volatile int r = setjmp(jb);
    if (r == 0)
        deep(10);
    check("setjmp/longjmp", r == 42);

    signal(SIGSEGV, on_signal);
    r = setjmp(jb);
    if (r == 0)
        *(volatile int *)16 = 1;
    check("SIGSEGV handler", r == SIGSEGV && sig_seen == SIGSEGV);
    signal(SIGSEGV, SIG_DFL);

    signal(SIGFPE, on_signal);
    volatile int zero = 0;
    r = setjmp(jb);
    if (r == 0)
        r = 10 / zero;
    check("SIGFPE handler", r == SIGFPE);
    signal(SIGFPE, SIG_DFL);

    /* Twice: the second fault must be delivered as well. */
    signal(SIGSEGV, on_signal);
    r = setjmp(jb);
    if (r == 0)
        *(volatile int *)24 = 1;
    check("SIGSEGV again", r == SIGSEGV);
    signal(SIGSEGV, SIG_DFL);

    sig_seen = 0;
    signal(SIGTERM, on_term);
    raise(SIGTERM);
    check("raise", sig_seen == SIGTERM);

    /* Ctrl+C sent to ourselves: the handler runs, then the program goes on. */
    sig_seen = 0;
    signal(SIGINT, on_term);
    APIRET rc = DosSendSignalException(getpid(), XCPT_SIGNAL_INTR);
    for (int i = 0; i < 100 && !sig_seen; i++)
        DosSleep(10);
    printf("    DosSendSignalException: %u\n", (unsigned)rc);
    check("SIGINT", rc == 0 && sig_seen == SIGINT);
    signal(SIGINT, SIG_DFL);
}

/* ---- time ---- */

static void test_time(void) {
    time_t now = time(0);
    check("time", now > 1600000000L);
    time_t t0 = 0;
    struct tm *g = gmtime(&t0);
    check("gmtime epoch", g->tm_year == 70 && g->tm_mon == 0 && g->tm_mday == 1 && g->tm_wday == 4 && g->tm_hour == 0);
    time_t t = 951782400 + 3723;   /* 2000-02-29 01:02:03 UTC */
    struct tm tm;
    gmtime_r(&t, &tm);
    char b[64];
    strftime(b, sizeof b, "%Y-%m-%d %H:%M:%S %a %j", &tm);
    check_str("strftime", b, "2000-02-29 01:02:03 Tue 060");
    setenv("TZ", "EST5EDT", 1);
    tzset();
    time_t summer = 1593000000;   /* 2020-06-24 12:00 UTC */
    struct tm *l = localtime(&summer);
    check("localtime EST5EDT", l->tm_hour == 8 && l->tm_isdst == 1);
    struct tm c = *l;
    check("mktime round trip", mktime(&c) == summer);
    unsetenv("TZ");
    tzset();
    clock_t c0 = clock();
    DosSleep(100);
    check("clock", clock() - c0 >= 50);
    printf("    %s", ctime(&now));
}

/* ---- files ---- */

static void test_files(void) {
    const char *name = "CLIBTEST.TMP";
    FILE *f = fopen(name, "w");
    check("fopen w", f != 0);
    if (!f)
        return;
    fprintf(f, "line one\nline %d\n", 2);
    fputs("third\n", f);
    check("fclose", fclose(f) == 0);
    f = fopen(name, "rb");
    char raw[64] = "";
    size_t n = fread(raw, 1, sizeof raw - 1, f);
    raw[n] = 0;
    fclose(f);
    check_str("text mode writes CR LF", raw, "line one\r\nline 2\r\nthird\r\n");
    f = fopen(name, "r");
    char line[64];
    fgets(line, sizeof line, f);
    check_str("text mode reads LF", line, "line one\n");
    long pos = ftell(f);
    check("ftell text", pos == 10);
    int num = 0;
    check("fscanf", fscanf(f, "line %d\n", &num) == 1 && num == 2);
    char *gl = 0;
    size_t cap = 0;
    check("getline", getline(&gl, &cap, f) == 6 && !strcmp(gl, "third\n"));
    free(gl);
    check("feof", fgetc(f) == EOF && feof(f));
    fseek(f, pos, SEEK_SET);
    fgets(line, sizeof line, f);
    check_str("fseek", line, "line 2\n");
    int c = fgetc(f);
    ungetc(c, f);
    check("ungetc", fgetc(f) == 't');
    fclose(f);
    f = fopen(name, "a");
    fputs("four\n", f);
    fclose(f);
    struct stat st;
    check("append + stat", stat(name, &st) == 0 && st.st_size == 31 && S_ISREG(st.st_mode));

    /* Binary: 300 KB through the buffer and around it. */
    f = fopen(name, "wb+");
    static unsigned char blk[300000];
    for (size_t i = 0; i < sizeof blk; i++)
        blk[i] = (unsigned char)(i * 7);
    check("fwrite", fwrite(blk, 1, sizeof blk, f) == sizeof blk);
    rewind(f);
    static unsigned char back[300000];
    check("fread", fread(back, 1, 100, f) == 100 && fread(back + 100, 1, sizeof back - 100, f) == sizeof back - 100);
    check("binary data", !memcmp(blk, back, sizeof blk));
    fseek(f, -10, SEEK_END);
    check("fseek end", ftell(f) == (long)sizeof blk - 10 && fgetc(f) == blk[sizeof blk - 10]);
    fclose(f);

    errno = 0;
    check("fopen missing", !fopen("NOSUCH.FIL", "r") && errno == ENOENT);
    check("rename", rename(name, "CLIBTST2.TMP") == 0 && access("CLIBTST2.TMP", F_OK) == 0 && access(name, F_OK) != 0);
    check("remove", remove("CLIBTST2.TMP") == 0 && access("CLIBTST2.TMP", F_OK) != 0);

    FILE *tf = tmpfile();
    check("tmpfile", tf && fputs("tmp", tf) >= 0 && fseek(tf, 0, SEEK_SET) == 0 && fgetc(tf) == 't');
    if (tf)
        fclose(tf);

    /* Handles. */
    int fd = open("CLIBFD.TMP", O_RDWR | O_CREAT | O_TRUNC, 0644);
    check("open", fd >= 0);
    check("write", write(fd, "abcdef", 6) == 6);
    check("lseek", lseek(fd, 2, SEEK_SET) == 2);
    char rb[8] = "";
    check("read", read(fd, rb, 3) == 3 && !memcmp(rb, "cde", 3));
    check("fstat", fstat(fd, &st) == 0 && st.st_size == 6);
    check("isatty file", !isatty(fd));
    close(fd);
    errno = 0;
    check("open O_EXCL", open("CLIBFD.TMP", O_RDWR | O_CREAT | O_EXCL, 0644) < 0 && errno == EEXIST);
    unlink("CLIBFD.TMP");

    /* Directories. */
    char cwd[260];
    check("getcwd", getcwd(cwd, sizeof cwd) && cwd[1] == ':');
    printf("    cwd %s\n", cwd);
    rmdir("CLIBDIR");
    check("mkdir", mkdir("CLIBDIR", 0755) == 0);
    errno = 0;
    check("mkdir exists", mkdir("CLIBDIR", 0755) < 0 && errno == EEXIST);
    fclose(fopen("CLIBDIR\\ONE.TXT", "w"));
    fclose(fopen("CLIBDIR\\TWO.TXT", "w"));
    DIR *d = opendir("CLIBDIR");
    int files = 0;
    struct dirent *e;
    while (d && (e = readdir(d)))
        if (e->d_type == DT_REG)
            files++;
    if (d)
        closedir(d);
    check("opendir/readdir", d && files == 2);
    check("chdir", chdir("CLIBDIR") == 0 && access("ONE.TXT", F_OK) == 0 && chdir("..") == 0);
    unlink("CLIBDIR\\ONE.TXT");
    unlink("CLIBDIR\\TWO.TXT");
    check("rmdir", rmdir("CLIBDIR") == 0);
}

/* ---- environment, threads, processes ---- */

static volatile int counter;
static volatile int errno_ok = 1;

static void worker(void *arg) {
    int id = (int)(intptr_t)arg;
    for (int i = 0; i < 200; i++) {
        errno = id * 1000 + i;
        char *p = malloc(100 + i);
        sprintf(p, "%d", i);
        DosSleep(0);
        if (errno != id * 1000 + i)
            errno_ok = 0;
        free(p);
    }
    __atomic_add_fetch(&counter, 1, __ATOMIC_SEQ_CST);
}

static int ctor_ran;
__attribute__((constructor)) static void ctor(void) {
    ctor_ran = 1;
}

static void test_env_threads(const char *self) {
    check("constructor", ctor_ran);
    check("getenv", getenv("PATH") != 0);
    check("setenv", setenv("CLIBTEST", "yes", 1) == 0 && !strcmp(getenv("CLIBTEST"), "yes"));
    check("unsetenv", unsetenv("CLIBTEST") == 0 && !getenv("CLIBTEST"));
    for (int i = 1; i <= 4; i++)
        _beginthread(worker, 0, 0, (void *)(intptr_t)i);
    for (int i = 0; i < 500 && counter < 4; i++)
        DosSleep(10);
    check("threads", counter == 4);
    check("errno per thread", errno_ok);

    setenv("CLIBVAR", "inherited", 1);
    char *argv[] = {(char *)self, "child", "two words", "quote\"inside", "back\\slash", 0};
    int r = spawnv(P_WAIT, self, argv);
    printf("    child returned %d\n", r);
    check("spawnv arguments and environment", r == 77);
    char cmd[300];
    snprintf(cmd, sizeof cmd, "%s child-out", self);
    FILE *p = popen(cmd, "r");
    char line[64] = "";
    if (p) {
        fgets(line, sizeof line, p);
        printf("    popen status %d\n", pclose(p));
    }
    check_str("popen", line, "hello from the child\n");
}

/* With LIBC64.DLL: a C DLL shares the program's C library. */
static void test_shared_dll(void) {
#ifdef _RS2_LIBC_DLL
    HMODULE h;
    char failed[64];
    APIRET rc = DosLoadModule((PSZ)failed, sizeof failed, (PCSZ) "CLIBTDLL", &h);
    check("DosLoadModule CLIBTDLL", rc == 0);
    if (rc)
        return;
    char *(*upper)(const char *) = 0;
    void (*print)(const char *) = 0;
    void (*set_errno)(int) = 0;
    DosQueryProcAddr(h, 0, (PCSZ) "dll_upper", (PPVOID)&upper);
    DosQueryProcAddr(h, 0, (PCSZ) "dll_print", (PPVOID)&print);
    DosQueryProcAddr(h, 0, (PCSZ) "dll_set_errno", (PPVOID)&set_errno);
    check("DosQueryProcAddr", upper && print && set_errno);
    if (!upper || !print || !set_errno)
        return;
    char *u = upper("shared heap");
    check("DLL's malloc, program's free", u && !strcmp(u, "SHARED HEAP"));
    free(u);
    printf("  shared stdout: ");
    print("ok");
    printf("\n");
    errno = 0;
    set_errno(4321);
    check("shared errno", errno == 4321);
#else
    printf("  (static C library: no shared DLL test)\n");
#endif
}

static int child(int argc, char **argv) {
    if (!strcmp(argv[1], "child-out")) {
        printf("hello from the child\n");
        return 0;
    }
    int ok = argc == 5 && !strcmp(argv[2], "two words") && !strcmp(argv[3], "quote\"inside") &&
             !strcmp(argv[4], "back\\slash");
    const char *v = getenv("CLIBVAR");
    ok = ok && v && !strcmp(v, "inherited");
    if (!ok)
        for (int i = 0; i < argc; i++)
            fprintf(stderr, "child argv[%d] = [%s]\n", i, argv[i]);
    return ok ? 77 : 1;
}

int main(int argc, char **argv) {
    if (argc > 1 && !strncmp(argv[1], "child", 5))
        return child(argc, argv);
#ifdef _RS2_LIBC_DLL
    printf("clibtest: rs2's 64-bit C library (LIBC64.DLL)\n");
#else
    printf("clibtest: rs2's 64-bit C library (static)\n");
#endif
    test_strings();
    test_printf();
    test_scanf();
    test_malloc();
    test_sort();
    test_math();
    test_signals();
    test_time();
    test_files();
    test_env_threads(argv[0]);
    test_shared_dll();
    printf("clibtest: %d checks, %d failures\n", checks, failures);
    return failures;
}
