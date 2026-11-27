/* GUI32: checks of rs2's USER32 and GDI32 (W32USER.DLL) from a Windows
 * GUI program built with MinGW-w64. Each check's result goes to the
 * debugger output (OutputDebugString: rs2's kernel log), then
 * "gui32 done: N failure(s) in M checks"; the exit code is N.
 *
 *     i686-w64-mingw32-windres gui32.rc -o gui32res.o
 *     i686-w64-mingw32-gcc -O1 -mwindows -o GUI32.EXE gui32.c gui32res.o
 */
#include <windows.h>
#include <stdio.h>
#include <string.h>
#include "gui32.h"

static int failures, checks;
static HINSTANCE inst;
static HWND main_wnd;

static void out(const char *s)
{
    OutputDebugStringA(s);
}

static void check(const char *what, int ok)
{
    char b[300];
    checks++;
    if (!ok)
        failures++;
    snprintf(b, sizeof b, "gui32 %s: %s", ok ? "ok" : "FAILED", what);
    out(b);
}

static void checkv(const char *what, int ok, long v)
{
    char b[300];
    snprintf(b, sizeof b, "%s (%ld)", what, v);
    check(b, ok);
}

/* ---- The main window's procedure ---- */

static int got_create, got_create_param, got_size, got_paint, got_timer, got_user2, got_command, got_destroy, got_ncdestroy;
static DWORD main_tid, proc_tid;
static LPARAM create_param;

static LRESULT CALLBACK main_proc(HWND h, UINT m, WPARAM wp, LPARAM lp)
{
    switch (m) {
    case WM_CREATE: {
        CREATESTRUCTA *cs = (CREATESTRUCTA *)lp;
        got_create++;
        create_param = (LPARAM)cs->lpCreateParams;
        got_create_param = cs->lpszName && strcmp(cs->lpszName, "GUI32 test") == 0;
        return 0;
    }
    case WM_SIZE:
        got_size++;
        return 0;
    case WM_PAINT: {
        PAINTSTRUCT ps;
        RECT r;
        HDC dc = BeginPaint(h, &ps);
        HBRUSH br = CreateSolidBrush(RGB(0, 128, 255));
        HFONT f, of;
        got_paint++;
        GetClientRect(h, &r);
        FillRect(dc, &r, (HBRUSH)(COLOR_WINDOW + 1));
        SetRect(&r, 10, 10, 110, 60);
        FillRect(dc, &r, br);
        DeleteObject(br);
        SelectObject(dc, GetStockObject(LTGRAY_BRUSH));
        Ellipse(dc, 130, 10, 230, 60);
        MoveToEx(dc, 10, 70, NULL);
        LineTo(dc, 230, 70);
        SetBkMode(dc, TRANSPARENT);
        f = CreateFontA(-16, 0, 0, 0, FW_BOLD, 0, 0, 0, ANSI_CHARSET, 0, 0, 0, VARIABLE_PITCH | FF_SWISS, "Arial");
        of = SelectObject(dc, f);
        TextOutA(dc, 10, 80, "Hello from Win32 on rs2", 23);
        SelectObject(dc, of);
        DeleteObject(f);
        SetRect(&r, 10, 110, 300, 140);
        DrawTextA(dc, "DrawText, centred", -1, &r, DT_CENTER | DT_SINGLELINE | DT_VCENTER);
        EndPaint(h, &ps);
        return 0;
    }
    case WM_TIMER:
        if (wp == 7)
            got_timer++;
        return 0;
    case WM_USER + 2:
        got_user2 += (wp == 5 && lp == 6);
        return 0;
    case WM_USER + 3:
        proc_tid = GetCurrentThreadId();
        return 0x1234;
    case WM_COMMAND:
        if (LOWORD(wp) == IDM_HELLO)
            got_command++;
        return 0;
    case WM_DESTROY:
        got_destroy++;
        return 0;
    case WM_NCDESTROY:
        got_ncdestroy++;
        break;
    }
    return DefWindowProcA(h, m, wp, lp);
}

/* ---- A W window ---- */

static int w_settext_ok;
static WCHAR w_seen[64];

static LRESULT CALLBACK w_proc(HWND h, UINT m, WPARAM wp, LPARAM lp)
{
    if (m == WM_SETTEXT && lp) {
        lstrcpynW(w_seen, (const WCHAR *)lp, 64);
        w_settext_ok = 1;
    }
    return DefWindowProcW(h, m, wp, lp);
}

/* ---- Subclassing ---- */

static WNDPROC old_proc;
static int sub_seen;

static LRESULT CALLBACK sub_proc(HWND h, UINT m, WPARAM wp, LPARAM lp)
{
    if (m == WM_USER + 1)
        sub_seen++;
    return CallWindowProcA(old_proc, h, m, wp, lp);
}

static DWORD WINAPI sender(LPVOID p)
{
    (void)p;
    return (DWORD)SendMessageA(main_wnd, WM_USER + 3, 0, 0);
}

/* Pump messages for `ms` milliseconds. */
static void pump(DWORD ms)
{
    DWORD start = GetTickCount();
    MSG msg;
    while (GetTickCount() - start < ms) {
        while (PeekMessageA(&msg, NULL, 0, 0, PM_REMOVE)) {
            TranslateMessage(&msg);
            DispatchMessageA(&msg);
        }
        Sleep(10);
    }
}

static void gdi_checks(void)
{
    HDC screen = GetDC(NULL), mem = CreateCompatibleDC(screen);
    HBITMAP bm = CreateCompatibleBitmap(screen, 16, 16), old;
    HBRUSH red = CreateSolidBrush(RGB(255, 0, 0));
    RECT r = { 0, 0, 16, 16 };
    COLORREF c;
    SIZE sz;
    TEXTMETRICA tm;
    HRGN a, b;
    BITMAPINFO bi;
    DWORD *bits = NULL;
    HBITMAP dib;
    HDC mem2;
    BYTE buf[16 * 16 * 4];
    BITMAP bmo;

    check("GetDC(NULL)", screen != NULL);
    check("CreateCompatibleDC", mem != NULL);
    old = SelectObject(mem, bm);
    check("SelectObject (bitmap)", old != NULL);
    FillRect(mem, &r, red);
    c = GetPixel(mem, 5, 5);
    checkv("FillRect then GetPixel is red", c == RGB(255, 0, 0), (long)c);
    SetPixel(mem, 3, 4, RGB(0, 255, 0));
    c = GetPixel(mem, 3, 4);
    checkv("SetPixel / GetPixel", c == RGB(0, 255, 0), (long)c);
    SelectObject(mem, GetStockObject(BLACK_PEN));
    MoveToEx(mem, 0, 10, NULL);
    LineTo(mem, 16, 10);
    c = GetPixel(mem, 8, 10);
    checkv("LineTo draws with the pen", c == RGB(0, 0, 0), (long)c);
    check("GetObject (BITMAP)", GetObjectA(bm, sizeof bmo, &bmo) == sizeof bmo && bmo.bmWidth == 16 && bmo.bmHeight == 16);

    /* GetDIBits: 32 bits per pel, top-down. */
    memset(&bi, 0, sizeof bi);
    bi.bmiHeader.biSize = sizeof bi.bmiHeader;
    bi.bmiHeader.biWidth = 16;
    bi.bmiHeader.biHeight = -16;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    SelectObject(mem, old);
    memset(buf, 0x55, sizeof buf);
    checkv("GetDIBits", GetDIBits(mem, bm, 0, 16, buf, &bi, DIB_RGB_COLORS) == 16, 0);
    checkv("GetDIBits: pel (5,5) red", ((DWORD *)buf)[5 * 16 + 5] == 0x00ff0000 || ((DWORD *)buf)[5 * 16 + 5] == 0xffff0000, (long)((DWORD *)buf)[5 * 16 + 5]);
    checkv("GetDIBits: pel (3,4) green", (((DWORD *)buf)[4 * 16 + 3] & 0xffffff) == 0x0000ff00, (long)((DWORD *)buf)[4 * 16 + 3]);

    /* A DIB section: the program's bits show in the bitmap, and what GDI
       draws shows in the bits. */
    dib = CreateDIBSection(screen, &bi, DIB_RGB_COLORS, (void **)&bits, NULL, 0);
    check("CreateDIBSection", dib != NULL && bits != NULL);
    if (dib && bits) {
        int i;
        for (i = 0; i < 256; i++)
            bits[i] = 0x000000ff; /* blue */
        bits[2 * 16 + 7] = 0x00ffff00; /* (7,2) yellow */
        mem2 = CreateCompatibleDC(screen);
        SelectObject(mem2, dib);
        c = GetPixel(mem2, 0, 0);
        checkv("DIB section: written bits read back (blue)", c == RGB(0, 0, 255), (long)c);
        c = GetPixel(mem2, 7, 2);
        checkv("DIB section: top-down row 2 (yellow)", c == RGB(255, 255, 0), (long)c);
        SelectObject(mem, bm);
        BitBlt(mem, 0, 0, 16, 16, mem2, 0, 0, SRCCOPY);
        c = GetPixel(mem, 7, 2);
        checkv("BitBlt from a DIB section", c == RGB(255, 255, 0), (long)c);
        SetRect(&r, 0, 0, 4, 4);
        FillRect(mem2, &r, red);
        GdiFlush();
        checkv("GDI drawing shows in a DIB section's bits", (bits[1 * 16 + 1] & 0xffffff) == 0x00ff0000, (long)bits[17]);
        DeleteDC(mem2);
        DeleteObject(dib);
    }

    /* Text. */
    SelectObject(mem, GetStockObject(SYSTEM_FONT));
    check("GetTextExtentPoint32A", GetTextExtentPoint32A(mem, "Hello", 5, &sz) && sz.cx > 0 && sz.cy > 0);
    check("GetTextMetricsA", GetTextMetricsA(mem, &tm) && tm.tmHeight > 0 && tm.tmAveCharWidth > 0);

    /* Regions. */
    a = CreateRectRgn(0, 0, 10, 10);
    b = CreateRectRgn(5, 5, 20, 20);
    checkv("CombineRgn RGN_OR", CombineRgn(a, a, b, RGN_OR) == COMPLEXREGION, 0);
    GetRgnBox(a, &r);
    check("GetRgnBox", r.left == 0 && r.top == 0 && r.right == 20 && r.bottom == 20);
    check("PtInRegion", PtInRegion(a, 15, 15) && !PtInRegion(a, 15, 2));
    DeleteObject(a);
    DeleteObject(b);

    DeleteObject(red);
    DeleteDC(mem);
    DeleteObject(bm);
    ReleaseDC(NULL, screen);
}

static void user_checks(void)
{
    char t[100];
    WCHAR wt[100];
    RECT r, w;
    POINT p = { 5, 7 };
    HMENU menu, sub;
    HWND child, wchild, sb;
    WNDCLASSW wc;
    SCROLLINFO si;
    MSG msg;
    HANDLE th;
    DWORD code = 0;

    check("WM_CREATE", got_create == 1);
    check("CREATESTRUCT lpCreateParams", create_param == 0x5150);
    check("CREATESTRUCT lpszName", got_create_param);
    GetWindowTextA(main_wnd, t, sizeof t);
    check("GetWindowTextA", strcmp(t, "GUI32 test") == 0);
    SetWindowTextA(main_wnd, "GUI32 title");
    GetWindowTextA(main_wnd, t, sizeof t);
    check("SetWindowTextA", strcmp(t, "GUI32 title") == 0);
    check("GetWindowTextLengthA", GetWindowTextLengthA(main_wnd) == 11);
    GetWindowTextW(main_wnd, wt, 100);
    check("GetWindowTextW", lstrcmpW(wt, L"GUI32 title") == 0);
    GetClientRect(main_wnd, &r);
    GetWindowRect(main_wnd, &w);
    checkv("GetClientRect", r.right > 100 && r.bottom > 100, r.right);
    check("GetWindowRect is bigger", w.right - w.left > r.right && w.bottom - w.top > r.bottom);
    ClientToScreen(main_wnd, &p);
    ScreenToClient(main_wnd, &p);
    check("ClientToScreen / ScreenToClient", p.x == 5 && p.y == 7);
    SetWindowLongA(main_wnd, GWL_USERDATA, 0x1234);
    check("GWL_USERDATA", GetWindowLongA(main_wnd, GWL_USERDATA) == 0x1234);
    SetWindowLongA(main_wnd, 4, 0xabcd);
    check("window extra bytes", GetWindowLongA(main_wnd, 4) == 0xabcd);
    check("GetWindowLong GWL_STYLE", (GetWindowLongA(main_wnd, GWL_STYLE) & WS_CAPTION) == WS_CAPTION);

    /* Subclassing. */
    old_proc = (WNDPROC)SetWindowLongA(main_wnd, GWL_WNDPROC, (LONG)sub_proc);
    check("SetWindowLong GWL_WNDPROC gives the old one", old_proc == main_proc);
    SendMessageA(main_wnd, WM_USER + 1, 0, 0);
    check("subclass procedure gets messages", sub_seen == 1);
    SetWindowLongA(main_wnd, GWL_WNDPROC, (LONG)old_proc);

    /* Child windows. */
    child = CreateWindowExA(0, "GUI32Child", "child", WS_CHILD | WS_VISIBLE, 10, 150, 100, 40, main_wnd, (HMENU)101, inst, NULL);
    check("child window", child != NULL);
    check("GetParent", GetParent(child) == main_wnd);
    check("GetDlgItem", GetDlgItem(main_wnd, 101) == child);
    check("GetDlgCtrlID", GetDlgCtrlID(child) == 101);
    check("IsChild", IsChild(main_wnd, child));
    GetWindowRect(child, &w);
    p.x = w.left;
    p.y = w.top;
    ScreenToClient(main_wnd, &p);
    checkv("child placed at (10,150)", p.x == 10 && p.y == 150, p.y);

    /* A W window: its procedure gets W text. */
    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = w_proc;
    wc.hInstance = inst;
    wc.lpszClassName = L"GUI32Wide";
    check("RegisterClassW", RegisterClassW(&wc) != 0);
    wchild = CreateWindowExW(0, L"GUI32Wide", L"wide", WS_CHILD, 0, 0, 10, 10, main_wnd, NULL, inst, NULL);
    check("CreateWindowExW", wchild != NULL);
    check("IsWindowUnicode", IsWindowUnicode(wchild) && !IsWindowUnicode(main_wnd));
    SendMessageA(wchild, WM_SETTEXT, 0, (LPARAM) "from A");
    check("A text reaches a W procedure as W", w_settext_ok && lstrcmpW(w_seen, L"from A") == 0);
    GetWindowTextA(wchild, t, sizeof t);
    check("GetWindowTextA of a W window", strcmp(t, "from A") == 0);

    /* Scroll bars. */
    sb = CreateWindowExA(0, "GUI32Child", "", WS_CHILD | WS_VSCROLL, 200, 150, 50, 80, main_wnd, NULL, inst, NULL);
    si.cbSize = sizeof si;
    si.fMask = SIF_RANGE | SIF_PAGE | SIF_POS;
    si.nMin = 0;
    si.nMax = 99;
    si.nPage = 10;
    si.nPos = 95;
    SetScrollInfo(sb, SB_VERT, &si, TRUE);
    memset(&si, 0, sizeof si);
    si.cbSize = sizeof si;
    si.fMask = SIF_ALL;
    GetScrollInfo(sb, SB_VERT, &si);
    checkv("SetScrollInfo / GetScrollInfo (position kept below max - page)", si.nMax == 99 && si.nPage == 10 && si.nPos == 90, si.nPos);

    /* Menus from the resources. */
    menu = GetMenu(main_wnd);
    check("GetMenu (the class's menu)", menu != NULL);
    checkv("GetMenuItemCount", GetMenuItemCount(menu) == 2, GetMenuItemCount(menu));
    sub = GetSubMenu(menu, 0);
    check("GetSubMenu", sub != NULL);
    GetMenuStringA(sub, IDM_HELLO, t, sizeof t, MF_BYCOMMAND);
    check("GetMenuStringA", strcmp(t, "&Hello") == 0);
    EnableMenuItem(menu, IDM_EXIT, MF_BYCOMMAND | MF_GRAYED);
    check("EnableMenuItem / GetMenuState", (GetMenuState(menu, IDM_EXIT, MF_BYCOMMAND) & MF_GRAYED) != 0);
    CheckMenuItem(menu, IDM_HELLO, MF_BYCOMMAND | MF_CHECKED);
    check("CheckMenuItem", (GetMenuState(menu, IDM_HELLO, MF_BYCOMMAND) & MF_CHECKED) != 0);
    check("AppendMenuA", AppendMenuA(sub, MF_STRING, 300, "&Added"));
    checkv("item count after AppendMenu", GetMenuItemCount(sub) == 4, GetMenuItemCount(sub));
    check("LoadAcceleratorsA", LoadAcceleratorsA(inst, MAKEINTRESOURCEA(IDA_MAIN)) != NULL);
    SendMessageA(main_wnd, WM_COMMAND, IDM_HELLO, 0);
    check("WM_COMMAND", got_command == 1);

    /* Strings. */
    LoadStringA(inst, IDS_HELLO, t, sizeof t);
    check("LoadStringA", strcmp(t, "Hello, resources") == 0);
    LoadStringW(inst, IDS_HELLO, wt, 100);
    check("LoadStringW", lstrcmpW(wt, L"Hello, resources") == 0);
    check("LoadIconA (the program's)", LoadIconA(inst, MAKEINTRESOURCEA(IDI_MAIN)) != NULL);
    check("LoadCursorA (IDC_ARROW)", LoadCursorA(NULL, IDC_ARROW) != NULL);

    /* Posting, the message loop, timers. */
    PostMessageA(main_wnd, WM_USER + 2, 5, 6);
    check("PeekMessage finds a posted message", PeekMessageA(&msg, main_wnd, WM_USER + 2, WM_USER + 2, PM_NOREMOVE) && msg.message == WM_USER + 2);
    pump(100);
    check("posted message dispatched", got_user2 == 1);
    SetTimer(main_wnd, 7, 30, NULL);
    pump(400);
    KillTimer(main_wnd, 7);
    checkv("WM_TIMER", got_timer > 0, got_timer);
    checkv("WM_PAINT", got_paint > 0, got_paint);
    check("WM_SIZE", got_size > 0);
    PostThreadMessageA(GetCurrentThreadId(), WM_USER + 9, 1, 2);
    check("PostThreadMessage", PeekMessageA(&msg, NULL, WM_USER + 9, WM_USER + 9, PM_REMOVE) && msg.hwnd == NULL && msg.wParam == 1);

    /* Another thread's SendMessage runs on the window's thread. */
    th = CreateThread(NULL, 0, sender, NULL, 0, NULL);
    pump(300);
    WaitForSingleObject(th, 2000);
    GetExitCodeThread(th, &code);
    CloseHandle(th);
    checkv("SendMessage from another thread: on the window's thread", proc_tid == main_tid, (long)proc_tid);
    checkv("SendMessage from another thread: the result", code == 0x1234, (long)code);

    /* The clipboard. */
    if (OpenClipboard(main_wnd)) {
        HGLOBAL g = GlobalAlloc(GMEM_MOVEABLE, 6);
        HANDLE d;
        memcpy(GlobalLock(g), "clip!", 6);
        GlobalUnlock(g);
        EmptyClipboard();
        SetClipboardData(CF_TEXT, g);
        d = GetClipboardData(CF_TEXT);
        check("clipboard text", d && strcmp((char *)GlobalLock(d), "clip!") == 0);
        CloseClipboard();
    } else {
        check("OpenClipboard", 0);
    }

    check("GetSystemMetrics(SM_CXSCREEN)", GetSystemMetrics(SM_CXSCREEN) > 0);
    check("GetSysColor(COLOR_WINDOW)", GetSysColor(COLOR_WINDOW) == RGB(255, 255, 255));
    r.left = 0, r.top = 0, r.right = 100, r.bottom = 100;
    AdjustWindowRect(&r, WS_OVERLAPPEDWINDOW, TRUE);
    check("AdjustWindowRect", r.left < 0 && r.top < -10 && r.right > 100);

    DestroyWindow(child);
    check("IsWindow after DestroyWindow", !IsWindow(child));
}

int WINAPI WinMain(HINSTANCE hi, HINSTANCE prev, LPSTR cmd, int show)
{
    WNDCLASSEXA wc;
    MSG msg;
    (void)prev;
    inst = hi;
    main_tid = GetCurrentThreadId();
    memset(&wc, 0, sizeof wc);
    wc.cbSize = sizeof wc;
    wc.style = CS_HREDRAW | CS_VREDRAW;
    wc.lpfnWndProc = main_proc;
    wc.cbWndExtra = 8;
    wc.hInstance = hi;
    wc.hIcon = LoadIconA(hi, MAKEINTRESOURCEA(IDI_MAIN));
    wc.hCursor = LoadCursorA(NULL, IDC_ARROW);
    wc.hbrBackground = (HBRUSH)(COLOR_WINDOW + 1);
    wc.lpszMenuName = MAKEINTRESOURCEA(IDM_MAIN);
    wc.lpszClassName = "GUI32Main";
    check("RegisterClassExA", RegisterClassExA(&wc) != 0);
    check("RegisterClassExA again fails", RegisterClassExA(&wc) == 0);
    /* The children's class: DefWindowProc is its procedure. */
    wc.lpfnWndProc = DefWindowProcA;
    wc.lpszMenuName = NULL;
    wc.hbrBackground = (HBRUSH)(COLOR_BTNFACE + 1);
    wc.lpszClassName = "GUI32Child";
    check("RegisterClassExA (DefWindowProc as the procedure)", RegisterClassExA(&wc) != 0);
    main_wnd = CreateWindowExA(0, "GUI32Main", "GUI32 test", WS_OVERLAPPEDWINDOW, 40, 40, 480, 320, NULL, NULL, hi, (LPVOID)0x5150);
    check("CreateWindowExA", main_wnd != NULL);
    ShowWindow(main_wnd, show);
    UpdateWindow(main_wnd);
    check("IsWindowVisible", IsWindowVisible(main_wnd));
    user_checks();
    gdi_checks();
    InvalidateRect(main_wnd, NULL, TRUE);
    pump(1500);
    /* GUI32 WAIT: the window stays until it is closed. */
    if (strstr(cmd, "wait")) {
        while (GetMessageA(&msg, NULL, 0, 0) > 0) {
            TranslateMessage(&msg);
            DispatchMessageA(&msg);
        }
    }
    {
        int d = got_destroy, n = got_ncdestroy;
        DestroyWindow(main_wnd);
        check("WM_DESTROY", got_destroy == d + 1);
        check("WM_NCDESTROY", got_ncdestroy == n + 1);
    }
    check("IsWindow after DestroyWindow (main)", !IsWindow(main_wnd));
    PostQuitMessage(failures);
    while (GetMessageA(&msg, NULL, 0, 0) > 0)
        DispatchMessageA(&msg);
    check("WM_QUIT's wParam", (int)msg.wParam == failures);
    {
        char b[100];
        snprintf(b, sizeof b, "gui32 done: %d failure(s) in %d checks", failures, checks);
        out(b);
    }
    return failures;
}
