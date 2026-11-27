/* W32TEST: checks of rs2's Win32 layer (docs/WIN32.md), stage 1.
   Prints a line per failed check and ends with "w32test done: N
   failure(s)"; the exit code is the number of failures. "W32TEST GROUP..."
   runs only the named groups: BASIC FILE MEM THREAD SEH PROC STR TIME
   CON DLL. */
#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <excpt.h>

/* W32DLL.DLL's exports (bound at load time). */
__declspec(dllimport) extern LONG W32DllAttaches;
__declspec(dllimport) extern LONG W32DllThreadAttaches;
int WINAPI W32DllAdd(int a, int b);
HMODULE WINAPI W32DllSelf(void);

static int failures = 0;
static int checks = 0;
static const char *group = "";

static void check(int ok, const char *what)
{
    checks++;
    if (!ok) {
        failures++;
        printf("FAIL %s: %s (GetLastError %lu)\n", group, what, GetLastError());
    }
}

/* ---- BASIC ---- */

static void t_basic(void)
{
    DWORD v = GetVersion();
    OSVERSIONINFOA vi;
    HANDLE out = GetStdHandle(STD_OUTPUT_HANDLE);
    DWORD n = 0;
    check((v & 0x80000000) == 0, "GetVersion says NT");
    check((v & 0xff) >= 4, "GetVersion major >= 4");
    vi.dwOSVersionInfoSize = sizeof vi;
    check(GetVersionExA(&vi), "GetVersionExA");
    check(vi.dwPlatformId == VER_PLATFORM_WIN32_NT, "platform NT");
    check(out != INVALID_HANDLE_VALUE && out != NULL, "GetStdHandle(output)");
    check(WriteFile(out, "", 0, &n, NULL), "WriteFile of nothing");
    SetLastError(1234);
    check(GetLastError() == 1234, "SetLastError/GetLastError");
    check(GetCurrentProcessId() != 0, "GetCurrentProcessId");
    check(GetCurrentThreadId() != 0, "GetCurrentThreadId");
    check(GetCommandLineA() != NULL && strstr(GetCommandLineA(), "W32TEST") != NULL || strstr(GetCommandLineA(), "w32test") != NULL, "GetCommandLineA has the program");
    check(GetModuleHandleA(NULL) == (HMODULE)0x400000, "GetModuleHandle(NULL) is the image base");
    check(GetModuleHandleA("KERNEL32.DLL") != NULL, "GetModuleHandle(KERNEL32)");
    check(GetProcAddress(GetModuleHandleA("KERNEL32"), "GetTickCount") == (FARPROC)GetTickCount, "GetProcAddress(GetTickCount)");
    check(GetProcAddress(GetModuleHandleA("KERNEL32"), "NoSuchFunction") == NULL, "GetProcAddress of a missing name");
}

/* ---- FILE ---- */

static void t_file(void)
{
    char buf[MAX_PATH], buf2[MAX_PATH];
    HANDLE h;
    DWORD n, attr;
    WIN32_FIND_DATAA fd;
    int found = 0;
    FILE *f;

    check(GetCurrentDirectoryA(sizeof buf, buf) > 0, "GetCurrentDirectoryA");
    check(GetTempPathA(sizeof buf2, buf2) > 0, "GetTempPathA");
    DeleteFileA("W32T.TMP");
    DeleteFileA("W32T2.TMP");
    h = CreateFileA("W32T.TMP", GENERIC_READ | GENERIC_WRITE, 0, NULL, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, NULL);
    check(h != INVALID_HANDLE_VALUE, "CreateFileA CREATE_NEW");
    check(GetFileType(h) == FILE_TYPE_DISK, "GetFileType disk");
    check(WriteFile(h, "hello, world", 12, &n, NULL) && n == 12, "WriteFile 12 bytes");
    check(GetFileSize(h, NULL) == 12, "GetFileSize");
    check(SetFilePointer(h, 7, NULL, FILE_BEGIN) == 7, "SetFilePointer");
    memset(buf, 0, sizeof buf);
    check(ReadFile(h, buf, sizeof buf, &n, NULL) && n == 5 && memcmp(buf, "world", 5) == 0, "ReadFile from 7");
    check(ReadFile(h, buf, sizeof buf, &n, NULL) && n == 0, "ReadFile at the end");
    check(CloseHandle(h), "CloseHandle");
    h = CreateFileA("W32T.TMP", GENERIC_READ, 0, NULL, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, NULL);
    check(h == INVALID_HANDLE_VALUE && GetLastError() == ERROR_FILE_EXISTS, "CREATE_NEW of an existing file");
    h = CreateFileA("NOSUCH.TMP", GENERIC_READ, 0, NULL, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, NULL);
    check(h == INVALID_HANDLE_VALUE && GetLastError() == ERROR_FILE_NOT_FOUND, "OPEN_EXISTING of a missing file");
    attr = GetFileAttributesA("W32T.TMP");
    check(attr != INVALID_FILE_ATTRIBUTES && !(attr & FILE_ATTRIBUTE_DIRECTORY), "GetFileAttributesA");
    check(MoveFileA("W32T.TMP", "W32T2.TMP"), "MoveFileA");
    check(GetFileAttributesA("W32T.TMP") == INVALID_FILE_ATTRIBUTES, "moved away");
    h = FindFirstFileA("W32T*.TMP", &fd);
    check(h != INVALID_HANDLE_VALUE, "FindFirstFileA");
    if (h != INVALID_HANDLE_VALUE) {
        do {
            if (_stricmp(fd.cFileName, "W32T2.TMP") == 0 && fd.nFileSizeLow == 12)
                found = 1;
        } while (FindNextFileA(h, &fd));
        check(GetLastError() == ERROR_NO_MORE_FILES, "FindNextFileA ends with ERROR_NO_MORE_FILES");
        check(FindClose(h), "FindClose");
    }
    check(found, "FindFirstFile found W32T2.TMP with its size");
    check(DeleteFileA("W32T2.TMP"), "DeleteFileA");
    check(CreateDirectoryA("W32TDIR", NULL), "CreateDirectoryA");
    attr = GetFileAttributesA("W32TDIR");
    check(attr != INVALID_FILE_ATTRIBUTES && (attr & FILE_ATTRIBUTE_DIRECTORY), "directory attributes");
    check(SetCurrentDirectoryA("W32TDIR"), "SetCurrentDirectoryA");
    check(GetCurrentDirectoryA(sizeof buf2, buf2) > 0 && strstr(buf2, "W32TDIR") != NULL, "in W32TDIR");
    check(SetCurrentDirectoryA(".."), "SetCurrentDirectoryA(..)");
    check(RemoveDirectoryA("W32TDIR"), "RemoveDirectoryA");
    check(GetFullPathNameA("X.TXT", sizeof buf2, buf2, NULL) > 0 && buf2[1] == ':' && strstr(buf2, "X.TXT") != NULL, "GetFullPathNameA");

    /* The C runtime's files. */
    f = fopen("W32T3.TMP", "w");
    check(f != NULL, "fopen w");
    if (f) {
        fprintf(f, "line one\nline two\n");
        fclose(f);
    }
    f = fopen("W32T3.TMP", "r");
    check(f != NULL && fgets(buf, sizeof buf, f) && strcmp(buf, "line one\n") == 0, "fopen r, fgets");
    if (f)
        fclose(f);
    check(remove("W32T3.TMP") == 0, "remove");
}

/* ---- MEM ---- */

static void t_mem(void)
{
    char *p;
    MEMORY_BASIC_INFORMATION mbi;
    HANDLE heap;
    void *a, *b;
    DWORD old;
    HGLOBAL g;

    p = VirtualAlloc(NULL, 0x20000, MEM_RESERVE, PAGE_NOACCESS);
    check(p != NULL && ((DWORD)p & 0xffff) == 0, "VirtualAlloc MEM_RESERVE (64 KB aligned)");
    check(VirtualAlloc(p, 0x1000, MEM_COMMIT, PAGE_READWRITE) == p, "VirtualAlloc MEM_COMMIT of a page");
    p[0] = 1;
    p[0xfff] = 2;
    check(VirtualQuery(p, &mbi, sizeof mbi) == sizeof mbi, "VirtualQuery");
    check(mbi.AllocationBase == p && mbi.State == MEM_COMMIT && mbi.RegionSize == 0x1000, "VirtualQuery: committed page");
    check(VirtualQuery(p + 0x1000, &mbi, sizeof mbi) == sizeof mbi && mbi.State == MEM_RESERVE, "VirtualQuery: reserved rest");
    check(VirtualProtect(p, 0x1000, PAGE_READONLY, &old) && old == PAGE_READWRITE, "VirtualProtect");
    check(VirtualFree(p, 0, MEM_RELEASE), "VirtualFree MEM_RELEASE");
    p = VirtualAlloc(NULL, 100000, MEM_COMMIT, PAGE_READWRITE);
    check(p != NULL, "VirtualAlloc MEM_COMMIT 100000");
    if (p) {
        p[99999] = 3;
        check(VirtualFree(p, 0, MEM_RELEASE), "VirtualFree");
    }
    heap = GetProcessHeap();
    check(heap != NULL, "GetProcessHeap");
    a = HeapAlloc(heap, HEAP_ZERO_MEMORY, 100);
    check(a != NULL && ((char *)a)[99] == 0, "HeapAlloc zeroed");
    check(HeapSize(heap, 0, a) >= 100, "HeapSize");
    b = HeapReAlloc(heap, 0, a, 5000);
    check(b != NULL && HeapSize(heap, 0, b) >= 5000, "HeapReAlloc");
    check(HeapFree(heap, 0, b), "HeapFree");
    heap = HeapCreate(0, 0x10000, 0);
    check(heap != NULL, "HeapCreate");
    if (heap) {
        a = HeapAlloc(heap, 0, 1000000);
        check(a != NULL, "HeapAlloc 1 MB in a private heap");
        check(HeapDestroy(heap), "HeapDestroy");
    }
    g = GlobalAlloc(GMEM_FIXED | GMEM_ZEROINIT, 64);
    check(g != NULL && ((char *)g)[63] == 0, "GlobalAlloc");
    check(GlobalFree(g) == NULL, "GlobalFree");
    a = LocalAlloc(LPTR, 32);
    check(a != NULL, "LocalAlloc");
    check(LocalFree(a) == NULL, "LocalFree");
    a = malloc(3000000);
    check(a != NULL, "malloc 3 MB");
    free(a);
}

/* ---- THREAD ---- */

static volatile LONG counter = 0;
static CRITICAL_SECTION cs;
static int shared_total = 0;
static DWORD tls_index;
static HANDLE go_event;

static DWORD WINAPI worker(LPVOID arg)
{
    int i;
    TlsSetValue(tls_index, arg);
    WaitForSingleObject(go_event, INFINITE);
    for (i = 0; i < 1000; i++) {
        InterlockedIncrement(&counter);
        EnterCriticalSection(&cs);
        shared_total++;
        LeaveCriticalSection(&cs);
    }
    return TlsGetValue(tls_index) == arg ? (DWORD)arg : 0;
}

static DWORD WINAPI deep(LPVOID arg)
{
    /* Grows the stack by 200 KB: guard pages, and the C runtime's probe. */
    char big[200000];
    (void)arg;
    big[0] = 1;
    big[sizeof big - 1] = 2;
    return big[0] + big[sizeof big - 1];
}

static void t_thread(void)
{
    HANDLE th[4], m, sem, ev;
    DWORD code, tid, r;
    int i, ok;

    InitializeCriticalSection(&cs);
    tls_index = TlsAlloc();
    check(tls_index != TLS_OUT_OF_INDEXES, "TlsAlloc");
    check(TlsSetValue(tls_index, (LPVOID)99) && TlsGetValue(tls_index) == (LPVOID)99, "TlsSetValue/TlsGetValue");
    go_event = CreateEventA(NULL, TRUE, FALSE, NULL);
    check(go_event != NULL, "CreateEventA (manual)");
    for (i = 0; i < 4; i++) {
        th[i] = CreateThread(NULL, 0, worker, (LPVOID)(i + 1), 0, &tid);
        check(th[i] != NULL && tid != 0, "CreateThread");
    }
    Sleep(50);
    check(counter == 0, "threads wait for the event");
    check(SetEvent(go_event), "SetEvent");
    r = WaitForMultipleObjects(4, th, TRUE, 20000);
    check(r < WAIT_OBJECT_0 + 4, "WaitForMultipleObjects (all)");
    check(counter == 4000, "InterlockedIncrement from four threads");
    check(shared_total == 4000, "critical sections");
    ok = 1;
    for (i = 0; i < 4; i++) {
        if (!GetExitCodeThread(th[i], &code) || code != (DWORD)(i + 1))
            ok = 0;
        CloseHandle(th[i]);
    }
    check(ok, "GetExitCodeThread (and TLS per thread)");
    check(TlsGetValue(tls_index) == (LPVOID)99, "thread 1's TLS value kept");
    check(TlsFree(tls_index), "TlsFree");
    DeleteCriticalSection(&cs);

    ev = CreateEventA(NULL, FALSE, TRUE, NULL);
    check(WaitForSingleObject(ev, 0) == WAIT_OBJECT_0, "auto-reset event signalled");
    check(WaitForSingleObject(ev, 0) == WAIT_TIMEOUT, "auto-reset event reset by the wait");
    check(WaitForSingleObject(ev, 100) == WAIT_TIMEOUT, "wait with a timeout");
    CloseHandle(ev);
    m = CreateMutexA(NULL, TRUE, NULL);
    check(m != NULL, "CreateMutexA owned");
    check(WaitForSingleObject(m, 0) == WAIT_OBJECT_0, "mutex is recursive for its owner");
    check(ReleaseMutex(m) && ReleaseMutex(m), "ReleaseMutex twice");
    check(!ReleaseMutex(m), "ReleaseMutex when not owned fails");
    CloseHandle(m);
    sem = CreateSemaphoreA(NULL, 2, 5, NULL);
    check(sem != NULL, "CreateSemaphoreA");
    check(WaitForSingleObject(sem, 0) == WAIT_OBJECT_0 && WaitForSingleObject(sem, 0) == WAIT_OBJECT_0, "two semaphore waits");
    check(WaitForSingleObject(sem, 0) == WAIT_TIMEOUT, "third wait times out");
    check(ReleaseSemaphore(sem, 1, (LONG *)&code) && code == 0, "ReleaseSemaphore");
    CloseHandle(sem);

    th[0] = CreateThread(NULL, 0, deep, NULL, 0, &tid);
    check(th[0] != NULL && WaitForSingleObject(th[0], 20000) == WAIT_OBJECT_0, "a thread with a deep stack ends");
    check(GetExitCodeThread(th[0], &code) && code == 3, "deep stack's result");
    CloseHandle(th[0]);
}

/* ---- SEH ---- */

static int finally_ran = 0;
static LONG filter_code = 0;

static LONG WINAPI top_filter(EXCEPTION_POINTERS *ep)
{
    (void)ep;
    return EXCEPTION_CONTINUE_SEARCH;
}

static int filter(DWORD code)
{
    filter_code = code;
    return EXCEPTION_EXECUTE_HANDLER;
}

static int raise_filter(EXCEPTION_POINTERS *ep, DWORD *n, LONG *i0, LONG *i1)
{
    *n = ep->ExceptionRecord->NumberParameters;
    *i0 = (LONG)ep->ExceptionRecord->ExceptionInformation[0];
    *i1 = (LONG)ep->ExceptionRecord->ExceptionInformation[1];
    return filter(ep->ExceptionRecord->ExceptionCode);
}

static void inner_finally(void)
{
    _try {
        RaiseException(0xE0001234, 0, 0, NULL);
    } _finally {
        finally_ran = 1;
    }
}

static volatile int zero = 0;
static volatile int *null_ptr = NULL;
static char *fix_target;
static char fix_page[16];

static LONG fixer(EXCEPTION_POINTERS *ep)
{
    if (ep->ExceptionRecord->ExceptionCode == EXCEPTION_ACCESS_VIOLATION && ep->ContextRecord->Eax == 0) {
        ep->ContextRecord->Eax = (DWORD)fix_page;
        return EXCEPTION_CONTINUE_EXECUTION;
    }
    return EXCEPTION_CONTINUE_SEARCH;
}

static void t_seh(void)
{
    int caught = 0, r = 0;
    ULONG_PTR args[2] = { 11, 22 };
    LONG info0 = 0, info1 = 0;
    DWORD nparams = 0;

    _try {
        *null_ptr = 1;
    } _except (filter(GetExceptionCode())) {
        caught = 1;
    }
    check(caught && filter_code == EXCEPTION_ACCESS_VIOLATION, "access violation caught by _except");

    caught = 0;
    _try {
        r = 10 / zero;
    } _except (filter(GetExceptionCode())) {
        caught = 1;
    }
    check(caught && filter_code == EXCEPTION_INT_DIVIDE_BY_ZERO, "divide by zero caught");

    caught = 0;
    _try {
        RaiseException(0xE0000001, 0, 2, args);
    } _except (raise_filter(GetExceptionInformation(), &nparams, &info0, &info1)) {
        caught = 1;
    }
    check(caught && filter_code == 0xE0000001, "RaiseException caught");
    check(nparams == 2 && info0 == 11 && info1 == 22, "RaiseException's parameters");

    caught = 0;
    finally_ran = 0;
    _try {
        inner_finally();
    } _except (EXCEPTION_EXECUTE_HANDLER) {
        caught = 1;
    }
    check(caught && finally_ran, "_finally runs while unwinding to _except");

    caught = 0;
    _try {
        _try {
            RaiseException(0xE0000002, 0, 0, NULL);
        } _except (EXCEPTION_CONTINUE_SEARCH) {
            caught = 2;
        }
    } _except (EXCEPTION_EXECUTE_HANDLER) {
        caught = caught == 0 ? 1 : 3;
    }
    check(caught == 1, "EXCEPTION_CONTINUE_SEARCH passes to the outer handler");

    /* EXCEPTION_CONTINUE_EXECUTION with a context the filter fixed. */
    r = 0;
    _try {
        _asm {
            xor eax, eax
            mov byte ptr [eax], 7
            mov r, eax
        }
    } _except (fixer(GetExceptionInformation())) {
        r = -1;
    }
    check(r == (int)fix_page && fix_page[0] == 7, "EXCEPTION_CONTINUE_EXECUTION with a changed context");

    check(SetUnhandledExceptionFilter(top_filter) != top_filter, "SetUnhandledExceptionFilter");
}

/* ---- PROC ---- */

static void t_proc(void)
{
    char buf[MAX_PATH];
    STARTUPINFOA si;
    PROCESS_INFORMATION pi;
    DWORD code = 0;
    SYSTEM_INFO sys;

    check(GetModuleFileNameA(NULL, buf, sizeof buf) > 0 && strstr(buf, "W32TEST.EXE") != NULL, "GetModuleFileNameA(NULL)");
    GetSystemInfo(&sys);
    check(sys.dwPageSize == 4096 && sys.dwAllocationGranularity == 65536 && sys.dwNumberOfProcessors >= 1, "GetSystemInfo");
    check(SetEnvironmentVariableA("W32TESTVAR", "value1"), "SetEnvironmentVariableA");
    check(GetEnvironmentVariableA("W32TESTVAR", buf, sizeof buf) == 6 && strcmp(buf, "value1") == 0, "GetEnvironmentVariableA");
    check(GetEnvironmentVariableA("W32TESTNONE", buf, sizeof buf) == 0 && GetLastError() == ERROR_ENVVAR_NOT_FOUND, "missing variable");
    check(getenv("W32TESTVAR") == NULL || strcmp(getenv("W32TESTVAR"), "value1") == 0, "getenv agrees");
    check(ExpandEnvironmentStringsA("[%W32TESTVAR%]", buf, sizeof buf) > 0 && strcmp(buf, "[value1]") == 0, "ExpandEnvironmentStringsA");
    memset(&si, 0, sizeof si);
    si.cb = sizeof si;
    GetModuleFileNameA(NULL, buf, sizeof buf);
    strcpy(strrchr(buf, '\\') + 1, "HELLO32.EXE one \"two three\"");
    check(CreateProcessA(NULL, buf, NULL, NULL, TRUE, 0, NULL, NULL, &si, &pi), "CreateProcessA HELLO32");
    if (pi.hProcess) {
        check(WaitForSingleObject(pi.hProcess, 30000) == WAIT_OBJECT_0, "WaitForSingleObject on a process");
        check(GetExitCodeProcess(pi.hProcess, &code) && code == 3, "GetExitCodeProcess is HELLO32's 3");
        CloseHandle(pi.hProcess);
        CloseHandle(pi.hThread);
    }
}

/* ---- STR ---- */

static void t_str(void)
{
    WCHAR w[64];
    char a[64];
    int n;

    n = MultiByteToWideChar(CP_ACP, 0, "Hello", -1, w, 64);
    check(n == 6 && w[0] == L'H' && w[5] == 0, "MultiByteToWideChar");
    n = WideCharToMultiByte(CP_ACP, 0, L"World", -1, a, sizeof a, NULL, NULL);
    check(n == 6 && strcmp(a, "World") == 0, "WideCharToMultiByte");
    check(lstrlenA("abc") == 3 && lstrlenW(L"abcd") == 4, "lstrlenA/W");
    lstrcpyA(a, "one");
    lstrcatA(a, "two");
    check(lstrcmpA(a, "onetwo") == 0, "lstrcpyA/lstrcatA/lstrcmpA");
    check(lstrcmpiA("ABC", "abc") == 0, "lstrcmpiA");
    strcpy(a, "mixed Case");
    CharUpperA(a);
    check(strcmp(a, "MIXED CASE") == 0, "CharUpperA");
    check(GetACP() == 1252 || GetACP() == 437 || GetACP() == 850, "GetACP");
    check(CompareStringA(LOCALE_USER_DEFAULT, NORM_IGNORECASE, "abc", -1, "ABC", -1) == CSTR_EQUAL, "CompareStringA");
}

/* ---- TIME ---- */

static void t_time(void)
{
    SYSTEMTIME st, lt;
    FILETIME ft;
    DWORD t0, t1;
    LARGE_INTEGER f, c0, c1;

    GetSystemTime(&st);
    check(st.wYear >= 2020 && st.wMonth >= 1 && st.wMonth <= 12, "GetSystemTime");
    GetLocalTime(&lt);
    check(lt.wYear >= 2020, "GetLocalTime");
    GetSystemTimeAsFileTime(&ft);
    check(ft.dwHighDateTime > 0x01d00000, "GetSystemTimeAsFileTime after 2015");
    check(FileTimeToSystemTime(&ft, &lt) && lt.wYear == st.wYear, "FileTimeToSystemTime");
    t0 = GetTickCount();
    Sleep(200);
    t1 = GetTickCount();
    check(t1 - t0 >= 150 && t1 - t0 < 2000, "Sleep(200) and GetTickCount");
    check(QueryPerformanceFrequency(&f) && f.QuadPart > 0, "QueryPerformanceFrequency");
    QueryPerformanceCounter(&c0);
    Sleep(20);
    QueryPerformanceCounter(&c1);
    check(c1.QuadPart > c0.QuadPart, "QueryPerformanceCounter goes on");
}

/* ---- CON ---- */

static void t_con(void)
{
    HANDLE out = GetStdHandle(STD_OUTPUT_HANDLE);
    CONSOLE_SCREEN_BUFFER_INFO bi;
    DWORD n, mode;
    COORD at;

    if (GetFileType(out) != FILE_TYPE_CHAR) {
        printf("(CON skipped: output is not a console)\n");
        return;
    }
    check(GetConsoleMode(out, &mode), "GetConsoleMode");
    check(GetConsoleScreenBufferInfo(out, &bi), "GetConsoleScreenBufferInfo");
    check(bi.dwSize.X >= 40 && bi.dwSize.Y >= 20, "screen buffer size");
    check(SetConsoleTextAttribute(out, FOREGROUND_GREEN | FOREGROUND_INTENSITY), "SetConsoleTextAttribute");
    check(WriteConsoleA(out, "green text\r\n", 12, &n, NULL) && n == 12, "WriteConsoleA");
    check(WriteConsoleW(out, L"wide text\r\n", 11, &n, NULL) && n == 11, "WriteConsoleW");
    SetConsoleTextAttribute(out, bi.wAttributes);
    check(GetConsoleScreenBufferInfo(out, &bi), "GetConsoleScreenBufferInfo again");
    at.X = 0;
    at.Y = bi.dwCursorPosition.Y;
    check(SetConsoleCursorPosition(out, at), "SetConsoleCursorPosition");
    check(FillConsoleOutputCharacterA(out, '*', 5, at, &n) && n == 5, "FillConsoleOutputCharacterA");
    check(SetConsoleTitleA("W32TEST"), "SetConsoleTitleA");
    printf("\n");
}

/* ---- DLL ---- */

static void t_dll(void)
{
    HMODULE h;
    int (WINAPI *add)(int, int);

    check(W32DllAttaches == 1, "W32DLL's DllMain ran once (DLL_PROCESS_ATTACH)");
    check(W32DllAdd(2, 3) == 5, "a call into W32DLL");
    h = GetModuleHandleA("W32DLL.DLL");
    check(h != NULL && h == W32DllSelf(), "GetModuleHandle(W32DLL) is its base");
    h = LoadLibraryA("W32DLL.DLL");
    check(h != NULL && h == W32DllSelf(), "LoadLibrary of a loaded DLL");
    /* Open Watcom exports stdcall names decorated, as Windows' own
       GetProcAddress needs them. */
    add = (int (WINAPI *)(int, int))GetProcAddress(h, "_W32DllAdd@8");
    check(add != NULL && add(4, 5) == 9, "GetProcAddress in W32DLL");
    check(W32DllAttaches == 1, "no second DLL_PROCESS_ATTACH");
    check(FreeLibrary(h), "FreeLibrary");
    check(LoadLibraryA("NOSUCH32.DLL") == NULL && GetLastError() == ERROR_MOD_NOT_FOUND, "LoadLibrary of a missing DLL");
}

struct group {
    const char *name;
    void (*fn)(void);
};

static struct group groups[] = {
    { "BASIC", t_basic }, { "FILE", t_file }, { "MEM", t_mem }, { "THREAD", t_thread },
    { "SEH", t_seh }, { "PROC", t_proc }, { "STR", t_str }, { "TIME", t_time },
    { "CON", t_con }, { "DLL", t_dll }, { NULL, NULL }
};

int main(int argc, char **argv)
{
    int i, j;
    setvbuf(stdout, NULL, _IONBF, 0);
    for (i = 0; groups[i].name; i++) {
        int run = argc < 2;
        for (j = 1; j < argc; j++)
            if (_stricmp(argv[j], groups[i].name) == 0)
                run = 1;
        if (!run)
            continue;
        group = groups[i].name;
        printf("w32test: %s\n", group);
        groups[i].fn();
    }
    printf("w32test done: %d failure(s) in %d checks\n", failures, checks);
    return failures;
}
