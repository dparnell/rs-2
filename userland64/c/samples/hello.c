/* CHELLO64.EXE: a 64-bit OS/2 program in C (docs/API64.md). It prints
 * its arguments and checks a few calls through the C headers: system
 * information, memory, a thread, an event semaphore, a file and an
 * exception handler. Exits with the number of failures. */

#include <os2.h>

static void out(const char *s) {
    ULONG n, len = 0;
    while (s[len])
        len++;
    DosWrite(1, s, len, &n);
}

static void num(unsigned long long v) {
    char b[24];
    int i = 23;
    b[i] = 0;
    do {
        b[--i] = '0' + v % 10;
        v /= 10;
    } while (v);
    out(b + i);
}

static int failures;

static void check(const char *what, int ok) {
    out("  ");
    out(what);
    out(ok ? ": ok\r\n" : ": FAILED\r\n");
    if (!ok)
        failures++;
}

static volatile ULONG thread_sum;

static void thread(ULONG_PTR param) {
    if (param == 0x123456789abcULL)
        thread_sum += 1;
}

static char *sparse;

static ULONG handler(PEXCEPTIONREPORTRECORD r, PEXCEPTIONREGISTRATIONRECORD reg, PCONTEXTRECORD c, PVOID d) {
    (void)reg, (void)c, (void)d;
    if (r->ExceptionNum == XCPT_ACCESS_VIOLATION && sparse &&
        r->ExceptionInfo[1] >= (ULONG_PTR)sparse && r->ExceptionInfo[1] < (ULONG_PTR)sparse + 65536) {
        DosSetMem((PVOID)(r->ExceptionInfo[1] & ~0xfffULL), 4096, PAG_COMMIT | PAG_READ | PAG_WRITE);
        return XCPT_CONTINUE_EXECUTION;
    }
    return XCPT_CONTINUE_SEARCH;
}

int main(int argc, char **argv) {
    out("Hello from 64-bit OS/2, in C!\r\n");
    for (int i = 0; i < argc; i++) {
        out("  argv[");
        num(i);
        out("] = ");
        out(argv[i]);
        out("\r\n");
    }
    check("sizeof(void *) == 8, sizeof(ULONG) == 4", sizeof(void *) == 8 && sizeof(ULONG) == 4 && sizeof(QMSG) == 40);

    ULONG v[2];
    check("DosQuerySysInfo", DosQuerySysInfo(QSV_VERSION_MAJOR, QSV_VERSION_MINOR, v, sizeof v) == NO_ERROR && v[0] == 20);

    PTIB tib;
    PPIB pib;
    DosGetInfoBlocks(&tib, &pib);
    check("DosGetInfoBlocks", tib->tib_ptib2->tib2_ultid == 1 && pib->pib_ulpid != 0);

    PVOID p;
    APIRET rc = DosAllocMem(&p, 1 << 20, PAG_READ | PAG_WRITE | PAG_COMMIT);
    check("DosAllocMem", rc == NO_ERROR && p != NULL);
    if (rc == NO_ERROR) {
        ((char *)p)[(1 << 20) - 1] = 1;
        DosFreeMem(p);
    }

    TID tid;
    rc = DosCreateThread(&tid, thread, 0x123456789abcULL, CREATE_READY | STACK_SPARSE, 65536);
    if (rc == NO_ERROR)
        DosWaitThread(&tid, DCWW_WAIT);
    check("DosCreateThread (64-bit parameter)", rc == NO_ERROR && thread_sum == 1);

    HEV hev;
    check("event semaphore", DosCreateEventSem(NULL, &hev, 0, FALSE) == NO_ERROR && DosPostEventSem(hev) == NO_ERROR &&
                                 DosWaitEventSem(hev, 1000) == NO_ERROR && DosCloseEventSem(hev) == NO_ERROR);

    HFILE hf;
    ULONG action, n;
    char buf[16];
    int ok = DosOpen("CHELLO64.TMP", &hf, &action, 0, FILE_NORMAL, OPEN_ACTION_CREATE_IF_NEW | OPEN_ACTION_REPLACE_IF_EXISTS,
                     OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYNONE, NULL) == NO_ERROR;
    ok = ok && DosWrite(hf, "from C", 6, &n) == NO_ERROR && n == 6;
    ok = ok && DosSetFilePtr(hf, 0, FILE_BEGIN, &n) == NO_ERROR;
    ok = ok && DosRead(hf, buf, sizeof buf, &n) == NO_ERROR && n == 6 && buf[5] == 'C';
    DosClose(hf);
    DosDelete("CHELLO64.TMP");
    check("a file", ok);

    EXCEPTIONREGISTRATIONRECORD rec = {0, handler};
    DosSetExceptionHandler(&rec);
    DosAllocMem((PPVOID)&sparse, 65536, PAG_READ | PAG_WRITE);
    sparse[0x4321] = 42;
    check("exception handler (sparse memory)", sparse[0x4321] == 42);
    DosUnsetExceptionHandler(&rec);
    DosFreeMem(sparse);

    out(failures ? "CHELLO64: tests failed\r\n" : "CHELLO64: all tests passed\r\n");
    return failures;
}
