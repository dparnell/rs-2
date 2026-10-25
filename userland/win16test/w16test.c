/* W16TEST: checks of rs2's native Win16 layer (docs/WIN16.md). A
 * Windows 3.x program that calls KERNEL, USER and GDI and the DOS API
 * and writes PASS/FAIL lines to standard output (INT 21h), ending with
 * "w16test done: N failure(s)". Run it with
 * cargo xtask run --release --headless --timeout 150 --init "RS2\W16TEST.EXE"
 */

#include <windows.h>
#include <string.h>
#include <stdio.h>
#include <io.h>
#include <fcntl.h>

static int failures;
static char line[160];

static void out(const char *s)
{
    write(1, s, strlen(s));
}

static void check(int ok, const char *what)
{
    out(ok ? "PASS " : "FAIL ");
    out(what);
    out("\r\n");
    if (!ok)
        failures++;
}

static int created, destroyed, user_msgs, posted_seen, timer_seen, painted;
static LONG create_param;

LONG FAR PASCAL _export TestWndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_CREATE:
        created++;
        create_param = (LONG)((CREATESTRUCT FAR *)lp)->lpCreateParams;
        return 0;
    case WM_USER:
        user_msgs++;
        return wp + 1;
    case WM_USER + 1:
        posted_seen++;
        return 0;
    case WM_TIMER:
        timer_seen++;
        return 0;
    case WM_PAINT: {
        PAINTSTRUCT ps;
        BeginPaint(hwnd, &ps);
        painted++;
        EndPaint(hwnd, &ps);
        return 0;
    }
    case WM_DESTROY:
        destroyed++;
        return 0;
    }
    return DefWindowProc(hwnd, msg, wp, lp);
}

static void kernel_tests(void)
{
    DWORD v = GetVersion();
    HLOCAL hl;
    HGLOBAL hg;
    char NEAR *p;
    char FAR *g;
    char huge *h;
    char buf[32];

    check(LOBYTE(LOWORD(v)) == 3 && HIBYTE(LOWORD(v)) == 10, "GetVersion is Windows 3.10");
    check((GetWinFlags() & WF_PMODE) != 0, "GetWinFlags: protected mode");

    hl = LocalAlloc(LMEM_MOVEABLE | LMEM_ZEROINIT, 100);
    p = LocalLock(hl);
    check(hl != 0 && p != NULL && p[0] == 0 && p[99] == 0, "LocalAlloc/LocalLock (zeroed)");
    check(LocalSize(hl) >= 100, "LocalSize");
    strcpy(p, "local");
    LocalUnlock(hl);
    hl = LocalReAlloc(hl, 2000, LMEM_MOVEABLE);
    p = LocalLock(hl);
    check(hl != 0 && strcmp(p, "local") == 0 && LocalSize(hl) >= 2000, "LocalReAlloc keeps the data");
    LocalUnlock(hl);
    check(LocalFree(hl) == 0, "LocalFree");

    hg = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, 100000L);
    g = GlobalLock(hg);
    check(hg != 0 && g != NULL, "GlobalAlloc 100000 bytes");
    check(GlobalSize(hg) >= 100000L, "GlobalSize");
    h = (char huge *)g;
    h[70000L] = 'x';
    check(h[70000L] == 'x' && h[69999L] == 0, "a huge pointer across 64 KB");
    GlobalUnlock(hg);
    hg = GlobalReAlloc(hg, 120000L, GMEM_MOVEABLE);
    h = (char huge *)GlobalLock(hg);
    check(hg != 0 && h[70000L] == 'x', "GlobalReAlloc keeps the data");
    GlobalUnlock(hg);
    check(GlobalFree(hg) == 0, "GlobalFree");

    lstrcpy(buf, "Win");
    lstrcat(buf, "16");
    check(lstrlen(buf) == 5 && strcmp(buf, "Win16") == 0, "lstrcpy, lstrcat, lstrlen");

    check(GetModuleHandle("USER") != 0 && GetProcAddress(GetModuleHandle("USER"), "MESSAGEBOX") != NULL,
          "GetModuleHandle(USER), GetProcAddress");
    check(GetModuleFileName(GetModuleHandle(NULL), buf, sizeof buf) > 0, "GetModuleFileName");
}

static void dos_tests(void)
{
    int fd;
    char buf[16];
    int n;

    fd = open("C:\\W16TEST.TMP", O_CREAT | O_TRUNC | O_RDWR | O_BINARY, 0666);
    check(fd >= 0, "INT 21h: create a file");
    if (fd < 0)
        return;
    check(write(fd, "hello, file", 11) == 11, "INT 21h: write");
    check(lseek(fd, 7L, SEEK_SET) == 7L, "INT 21h: seek");
    n = read(fd, buf, sizeof buf);
    check(n == 4 && memcmp(buf, "file", 4) == 0, "INT 21h: read");
    close(fd);
    check(unlink("C:\\W16TEST.TMP") == 0, "INT 21h: delete");
}

static void user_tests(HINSTANCE hInst)
{
    WNDCLASS wc;
    HWND hwnd;
    RECT rc;
    POINT pt;
    MSG msg;
    HDC hdc;
    TEXTMETRIC tm;
    DWORD ext;

    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = TestWndProc;
    wc.hInstance = hInst;
    wc.hCursor = LoadCursor(NULL, IDC_ARROW);
    wc.hbrBackground = GetStockObject(WHITE_BRUSH);
    wc.lpszClassName = "W16Test";
    wc.cbWndExtra = 4;
    check(RegisterClass(&wc) != 0, "RegisterClass");
    check(RegisterClass(&wc) == 0, "RegisterClass twice fails");

    hwnd = CreateWindow("W16Test", "W16TEST", WS_OVERLAPPEDWINDOW, 100, 80, 300, 200,
                        NULL, NULL, hInst, (LPVOID)0x12345678L);
    check(hwnd != 0, "CreateWindow");
    check(created == 1 && create_param == 0x12345678L, "WM_CREATE with the CREATESTRUCT");
    GetClientRect(hwnd, &rc);
    check(rc.left == 0 && rc.top == 0 && rc.right > 250 && rc.right < 300 && rc.bottom > 120 && rc.bottom < 200,
          "GetClientRect: the client area of a 300x200 window");
    GetWindowRect(hwnd, &rc);
    sprintf(line, "GetWindowRect: (100,80)-(400,280), is (%d,%d)-(%d,%d)", rc.left, rc.top, rc.right, rc.bottom);
    check(rc.left == 100 && rc.top == 80 && rc.right == 400 && rc.bottom == 280, line);

    pt.x = 10;
    pt.y = 20;
    ClientToScreen(hwnd, &pt);
    check(pt.x > 100 && pt.y > 80 + 20, "ClientToScreen: y grows down");
    ScreenToClient(hwnd, &pt);
    check(pt.x == 10 && pt.y == 20, "ScreenToClient undoes it");

    check(SetWindowLong(hwnd, 0, 0xCAFEL) == 0 && GetWindowLong(hwnd, 0) == 0xCAFEL, "window extra bytes");
    check(GetWindowLong(hwnd, GWL_WNDPROC) == (LONG)(FARPROC)TestWndProc, "GWL_WNDPROC");

    check(SendMessage(hwnd, WM_USER, 41, 0) == 42 && user_msgs == 1, "SendMessage");
    check(PostMessage(hwnd, WM_USER + 1, 0, 0), "PostMessage");
    while (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE))
        DispatchMessage(&msg);
    check(posted_seen == 1, "PeekMessage and DispatchMessage of a posted message");

    ShowWindow(hwnd, SW_SHOWNORMAL);
    UpdateWindow(hwnd);
    check(painted > 0, "UpdateWindow sends WM_PAINT");
    check(IsWindowVisible(hwnd), "ShowWindow");

    hdc = GetDC(hwnd);
    check(hdc != 0, "GetDC");
    check(GetTextMetrics(hdc, &tm) && tm.tmHeight > 8, "GetTextMetrics");
    ext = GetTextExtent(hdc, "Hello", 5);
    check(LOWORD(ext) > 10 && HIWORD(ext) == (WORD)tm.tmHeight, "GetTextExtent");
    check(TextOut(hdc, 5, 5, "W16TEST", 7), "TextOut");
    SetRect(&rc, 0, 0, 100, 0);
    DrawText(hdc, "two\nlines", -1, &rc, DT_CALCRECT);
    check(rc.bottom == 2 * tm.tmHeight, "DrawText DT_CALCRECT: two lines");
    ReleaseDC(hwnd, hdc);

    check(SetTimer(hwnd, 7, 100, NULL) != 0, "SetTimer");
    while (!timer_seen && GetMessage(&msg, NULL, 0, 0))
        DispatchMessage(&msg);
    KillTimer(hwnd, 7);
    check(timer_seen > 0, "WM_TIMER through GetMessage");

    check(DestroyWindow(hwnd) && destroyed == 1, "DestroyWindow sends WM_DESTROY");
    check(!IsWindow(hwnd), "IsWindow after DestroyWindow");

    PostQuitMessage(3);
    check(!GetMessage(&msg, NULL, 0, 0) && msg.message == WM_QUIT && msg.wParam == 3, "PostQuitMessage: GetMessage returns 0");
}

int PASCAL WinMain(HINSTANCE hInst, HINSTANCE hPrev, LPSTR cmd, int show)
{
    out("w16test: rs2's native Win16 layer\r\n");
    kernel_tests();
    dos_tests();
    user_tests(hInst);
    sprintf(line, "w16test done: %d failure(s)\r\n", failures);
    out(line);
    return failures;
}
