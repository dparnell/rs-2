/* MTEST: checks of rs2's MSVCRT.DLL (docs/WIN32.md) with a MinGW-w64
   program: the C library as MinGW programs import it from MSVCRT.DLL.
   Ends with "mtest done: N failure(s) in M checks"; the exit code is N. */
#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <ctype.h>
#include <math.h>
#include <time.h>
#include <setjmp.h>
#include <errno.h>
#include <process.h>
#include <io.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <locale.h>
#include <wchar.h>
#include <signal.h>
#include <direct.h>

static int failures, checks;

static void check(int ok, const char *what)
{
    checks++;
    if (!ok) {
        failures++;
        printf("FAIL %s\n", what);
    }
}

static int cmp(const void *a, const void *b) { return *(const int *)a - *(const int *)b; }

static jmp_buf jb;
static void jump(int v) { longjmp(jb, v); }

static volatile LONG thread_sum;
static unsigned __stdcall worker(void *arg)
{
    InterlockedExchangeAdd(&thread_sum, (LONG)(INT_PTR)arg);
    return (unsigned)(INT_PTR)arg * 2;
}

static int atexit_ran;
static void on_exit_fn(void) { printf("mtest: atexit ran\n"); }

int main(int argc, char **argv, char **envp)
{
    char buf[256];
    FILE *f;
    int i, a[6] = { 5, 3, 9, 1, 7, 2 };
    double d;
    char *end, *p;
    struct stat st;
    time_t t;
    struct tm *tm;
    HANDLE th;
    unsigned code;
    int fd;

    setvbuf(stdout, NULL, _IONBF, 0);
    printf("mtest: %d argument(s), argv[0] %s\n", argc, argv[0]);
    check(argc >= 1 && argv[argc] == NULL, "argv ends with NULL");
    check(argc < 3 || strcmp(argv[1], "one") == 0, "argv[1]");
    check(envp != NULL && envp[0] != NULL, "envp");
    check(getenv("PATH") != NULL, "getenv PATH");
    check(_putenv("MTESTVAR=xyz") == 0 && getenv("MTESTVAR") && strcmp(getenv("MTESTVAR"), "xyz") == 0, "_putenv/getenv");

    /* Strings and memory. */
    snprintf(buf, sizeof buf, "%d %5.2f %s %x %c %05d %-3s|", -42, 3.14159, "str", 255, 'Z', 42, "ab");
    check(strcmp(buf, "-42  3.14 str ff Z 00042 ab |") == 0, "snprintf");
    printf("  snprintf: [%s]\n", buf);
    check(strlen("hello") == 5 && strncmp("abc", "abd", 2) == 0 && strchr("abc", 'c') && strrchr("abca", 'a') && strstr("haystack", "st"), "string functions");
    check(_stricmp("ABC", "abc") == 0 && _strnicmp("ABx", "aby", 2) == 0, "_stricmp");
    strcpy(buf, "Hello");
    strcat(buf, " World");
    check(strcmp(buf, "Hello World") == 0, "strcpy/strcat");
    check(toupper('a') == 'A' && isdigit('5') && isspace(' ') && isalpha('q') && !isalpha('1'), "ctype");
    p = _strdup("dup");
    check(p && strcmp(p, "dup") == 0, "_strdup");
    free(p);
    p = malloc(1000);
    p = realloc(p, 100000);
    check(p != NULL, "malloc/realloc");
    memset(p, 0x5a, 100000);
    check(p[99999] == 0x5a, "memset");
    free(p);
    p = calloc(10, 10);
    check(p && p[99] == 0, "calloc");
    free(p);
    qsort(a, 6, sizeof a[0], cmp);
    check(a[0] == 1 && a[5] == 9, "qsort");
    check(atoi("123") == 123 && strtol("-ff", &end, 16) == -255 && strtoul("42x", &end, 10) == 42 && *end == 'x', "atoi/strtol");
    d = strtod("2.5e3", &end);
    check(d == 2500.0, "strtod");
    check(atof("1.5") == 1.5, "atof");
    check(fabs(sqrt(2.0) - 1.41421356) < 1e-6 && fabs(sin(0.0)) < 1e-12 && floor(2.7) == 2.0 && pow(2, 10) == 1024, "math");
    check(abs(-3) == 3 && labs(-4L) == 4, "abs");
    sprintf(buf, "%g %e", 0.5, 12345.678);
    check(strcmp(buf, "0.5 1.234568e+004") == 0 || strcmp(buf, "0.5 1.234568e+04") == 0, "sprintf %g %e");
    printf("  sprintf float: [%s]\n", buf);
    check(sscanf("12 abc 3.5", "%d %s %lf", &i, buf, &d) == 3 && i == 12 && strcmp(buf, "abc") == 0 && d == 3.5, "sscanf");

    /* Files. */
    f = fopen("MTEST.TMP", "w");
    check(f != NULL, "fopen w");
    if (f) {
        fprintf(f, "line1\nline2\n");
        fputs("line3\n", f);
        fclose(f);
    }
    f = fopen("MTEST.TMP", "r");
    check(f && fgets(buf, sizeof buf, f) && strcmp(buf, "line1\n") == 0, "fopen r, fgets (text mode)");
    if (f) {
        fseek(f, 0, SEEK_END);
        check(ftell(f) == 21, "ftell in text mode (CR LF on disk)");
        rewind(f);
        i = 0;
        while (fgets(buf, sizeof buf, f))
            i++;
        check(i == 3 && feof(f), "three lines and EOF");
        fclose(f);
    }
    check(stat("MTEST.TMP", &st) == 0 && st.st_size == 21, "stat");
    fd = _open("MTEST.TMP", _O_RDONLY | _O_BINARY);
    check(fd >= 0 && _read(fd, buf, 7) == 7 && memcmp(buf, "line1\r\n", 7) == 0, "_open/_read (binary)");
    if (fd >= 0)
        _close(fd);
    check(rename("MTEST.TMP", "MTEST2.TMP") == 0, "rename");
    check(_access("MTEST2.TMP", 0) == 0, "_access");
    check(remove("MTEST2.TMP") == 0 && fopen("MTEST2.TMP", "r") == NULL && errno == ENOENT, "remove, errno ENOENT");
    check(_getcwd(buf, sizeof buf) != NULL && buf[1] == ':', "_getcwd");
    check(_mkdir("MTESTD") == 0 && _rmdir("MTESTD") == 0, "_mkdir/_rmdir");

    /* Time. */
    t = time(NULL);
    check(t > 1600000000, "time");
    tm = localtime(&t);
    check(tm && tm->tm_year >= 120, "localtime");
    strftime(buf, sizeof buf, "%Y-%m-%d %H:%M", tm);
    printf("  now: %s\n", buf);
    check(clock() >= 0, "clock");

    /* setjmp/longjmp. */
    i = setjmp(jb);
    if (i == 0)
        jump(7);
    check(i == 7, "setjmp/longjmp");

    /* Threads through the C runtime. */
    th = (HANDLE)_beginthreadex(NULL, 0, worker, (void *)21, 0, NULL);
    check(th != NULL, "_beginthreadex");
    if (th) {
        WaitForSingleObject(th, 10000);
        check(GetExitCodeThread(th, (DWORD *)&code) && code == 42, "thread result");
        CloseHandle(th);
    }
    check(thread_sum == 21, "thread ran");

    /* Wide characters. */
    check(wcslen(L"wide") == 4 && wcscmp(L"a", L"b") < 0, "wcslen/wcscmp");
    swprintf(L"" == NULL ? NULL : (wchar_t *)buf, 32, L"%d", 5);
    check(((wchar_t *)buf)[0] == L'5', "swprintf");

    /* system() through the command interpreter. */
    check(system(NULL) != 0, "system(NULL): a command interpreter");
    atexit(on_exit_fn);
    printf("mtest done: %d failure(s) in %d checks\n", failures, checks);
    return failures;
}
