/* DDECLI16: a DDE client written with the WM_DDE_* messages themselves,
 * for DDESRV16 (a DDEML server in another task): it initiates a
 * conversation with RS2DDE|Clock, requests, pokes, executes, starts and
 * stops an advise loop, is refused an unknown item and terminates,
 * checking each answer. Writes PASS/FAIL lines to standard output and
 * "ddecli16 done: N failure(s)". Start DDESRV16 first (START /PM
 * RS2\DDESRV16.EXE), then this. A test of rs2's DDE between Windows
 * tasks (docs/WIN16.md). */

#include <windows.h>
#include <dde.h>
#include <string.h>
#include <stdio.h>
#include <io.h>

static HWND hMain, server;
static int failures, ndata, nack;
static WORD ack_status;
static HGLOBAL ack_hi;
static char data[128], line[200];
static ATOM data_item;
static BOOL terminated, advised;
static char shown[16][100];
static int nshown;

static void out(const char *s)
{
    write(1, s, strlen(s));
    if (nshown < 16)
        lstrcpy(shown[nshown++], s);
    if (hMain)
        InvalidateRect(hMain, NULL, TRUE);
}

static void check(int ok, const char *what)
{
    wsprintf(line, "%s %s\r\n", (LPSTR)(ok ? "PASS" : "FAIL"), (LPSTR)what);
    out(line);
    if (!ok)
        failures++;
}

/* Run the message loop until *flag changes from `was` or ms pass. */
static BOOL wait_for(int *flag, int was, DWORD ms)
{
    DWORD start = GetTickCount();
    MSG msg;
    while (*flag == was && GetTickCount() - start < ms) {
        if (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE)) {
            TranslateMessage(&msg);
            DispatchMessage(&msg);
        } else {
            Yield();
        }
    }
    return *flag != was;
}

/* Messages for ms milliseconds. */
static void delay(DWORD ms)
{
    int never = 0;
    wait_for(&never, 0, ms);
}

static void post(UINT msg, WORD lo, WORD hi)
{
    if (!PostMessage(server, msg, (WPARAM)hMain, MAKELONG(lo, hi)))
        check(0, "PostMessage to the server");
}

/* WM_DDE_REQUEST: the data (TRUE) or a negative WM_DDE_ACK. */
static BOOL request(const char *item)
{
    int n = ndata, a = nack;
    DWORD start = GetTickCount();
    post(WM_DDE_REQUEST, CF_TEXT, GlobalAddAtom(item));
    while (ndata == n && nack == a && GetTickCount() - start < 5000)
        wait_for(&ndata, n, 50);
    return ndata != n;
}

/* A DDESHARE block: flags, format, text (WM_DDE_POKE, WM_DDE_ADVISE). */
static HGLOBAL block(WORD flags, const char *text)
{
    HGLOBAL h = GlobalAlloc(GMEM_MOVEABLE | GMEM_DDESHARE, 4 + (text ? lstrlen(text) + 1 : 0));
    WORD FAR *p = (WORD FAR *)GlobalLock(h);
    p[0] = flags;
    p[1] = CF_TEXT;
    if (text)
        lstrcpy((LPSTR)(p + 2), text);
    GlobalUnlock(h);
    return h;
}

static BOOL wait_ack(void)
{
    int a = nack;
    return wait_for(&nack, a, 5000);
}

static BOOL execute(const char *cmd)
{
    HGLOBAL h = GlobalAlloc(GMEM_MOVEABLE | GMEM_DDESHARE, lstrlen(cmd) + 1);
    int a = nack;
    lstrcpy(GlobalLock(h), cmd);
    GlobalUnlock(h);
    post(WM_DDE_EXECUTE, 0, (WORD)h);
    if (!wait_for(&nack, a, 5000))
        return FALSE;
    /* The server gives the commands back with the ACK. */
    GlobalFree(ack_hi);
    return (ack_status & 0x8000) != 0;
}

static void run(void)
{
    ATOM app = GlobalAddAtom("RS2DDE"), topic = GlobalAddAtom("Clock");
    int tries, n;
    HGLOBAL h;

    out("ddecli16: DDE with DDESRV16\r\n");
    for (tries = 0; tries < 40 && !server; tries++) {
        SendMessage((HWND)0xffff, WM_DDE_INITIATE, (WPARAM)hMain, MAKELONG(app, topic));
        if (!server)
            delay(250);
    }
    check(server != 0, "WM_DDE_INITIATE: DDESRV16 answers with WM_DDE_ACK");
    if (!server)
        return;

    check(request("Time") && lstrlen(data) == 8 && data[2] == ':' && data[5] == ':', "WM_DDE_REQUEST Time: WM_DDE_DATA with hh:mm:ss");
    wsprintf(line, "  Time is %s\r\n", (LPSTR)data);
    out(line);

    /* fRelease: the server frees it. */
    h = block(0x2000, "Hello from DDECLI16");
    post(WM_DDE_POKE, (WORD)h, GlobalAddAtom("Text"));
    check(wait_ack() && (ack_status & 0x8000), "WM_DDE_POKE Text: a positive WM_DDE_ACK");
    check(request("Text") && lstrcmp(data, "Hello from DDECLI16") == 0, "WM_DDE_REQUEST Text: the text poked");

    check(execute("[set 41]") && execute("[inc]"), "WM_DDE_EXECUTE [set 41] and [inc]: positive ACKs");
    check(request("Counter") && lstrcmp(data, "42") == 0, "WM_DDE_REQUEST Counter: 42");

    /* An advise loop with acknowledgements (fAckReq). */
    h = block(0x8000, NULL);
    post(WM_DDE_ADVISE, (WORD)h, GlobalAddAtom("Counter"));
    check(wait_ack() && (ack_status & 0x8000), "WM_DDE_ADVISE Counter: a positive WM_DDE_ACK");
    advised = TRUE;
    n = ndata;
    execute("[inc]");
    wait_for(&ndata, n, 5000);
    check(ndata != n && lstrcmp(data, "43") == 0, "the advise loop sends WM_DDE_DATA 43 after [inc]");

    check(!request("Nope") && (ack_status & 0x8000) == 0, "WM_DDE_REQUEST of an unknown item: a negative WM_DDE_ACK");

    post(WM_DDE_UNADVISE, CF_TEXT, GlobalAddAtom("Counter"));
    check(wait_ack() && (ack_status & 0x8000), "WM_DDE_UNADVISE Counter: a positive WM_DDE_ACK");
    advised = FALSE;
    n = ndata;
    execute("[inc]");
    check(!wait_for(&ndata, n, 1000), "no WM_DDE_DATA after the advise loop stopped");

    check(execute("[quit]"), "WM_DDE_EXECUTE [quit]");
    post(WM_DDE_TERMINATE, 0, 0);
    wait_for(&terminated, FALSE, 5000);
    check(terminated, "WM_DDE_TERMINATE answered with WM_DDE_TERMINATE");
    GlobalDeleteAtom(app);
    GlobalDeleteAtom(topic);
}

LONG FAR PASCAL _export CliWndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_DDE_ACK:
        if (!server) {
            /* The answer to WM_DDE_INITIATE (sent). */
            server = (HWND)wp;
            GlobalDeleteAtom(LOWORD(lp));
            GlobalDeleteAtom(HIWORD(lp));
            return 0;
        }
        if ((HWND)wp != server) {
            /* A second server: not wanted. */
            PostMessage((HWND)wp, WM_DDE_TERMINATE, (WPARAM)hwnd, 0);
            return 0;
        }
        ack_status = LOWORD(lp);
        ack_hi = (HGLOBAL)HIWORD(lp);
        nack++;
        return 0;
    case WM_DDE_DATA: {
        HGLOBAL h = (HGLOBAL)LOWORD(lp);
        WORD FAR *p = (WORD FAR *)GlobalLock(h);
        WORD flags = p[0];
        lstrcpy(data, (LPSTR)(p + 2));
        GlobalUnlock(h);
        data_item = HIWORD(lp);
        /* fAckReq: acknowledge (with the item); fRelease: ours to free. */
        if (flags & 0x8000)
            PostMessage((HWND)wp, WM_DDE_ACK, (WPARAM)hwnd, MAKELONG(0x8000, data_item));
        else
            GlobalDeleteAtom(data_item);
        if (flags & 0x2000)
            GlobalFree(h);
        ndata++;
        return 0;
    }
    case WM_DDE_TERMINATE:
        terminated = TRUE;
        return 0;
    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC dc = BeginPaint(hwnd, &ps);
        TEXTMETRIC tm;
        int i;
        GetTextMetrics(dc, &tm);
        for (i = 0; i < nshown; i++) {
            int n = lstrlen(shown[i]);
            while (n && (shown[i][n - 1] == '\n' || shown[i][n - 1] == '\r'))
                n--;
            TextOut(dc, 8, 6 + i * tm.tmHeight, shown[i], n);
        }
        EndPaint(hwnd, &ps);
        return 0;
    }
    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProc(hwnd, msg, wp, lp);
}

int PASCAL WinMain(HINSTANCE hInst, HINSTANCE hPrev, LPSTR cmd, int show)
{
    WNDCLASS wc;
    MSG msg;
    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = CliWndProc;
    wc.hInstance = hInst;
    wc.hIcon = LoadIcon(NULL, IDI_APPLICATION);
    wc.hCursor = LoadCursor(NULL, IDC_ARROW);
    wc.hbrBackground = COLOR_WINDOW + 1;
    wc.lpszClassName = "DdeCli16";
    RegisterClass(&wc);
    hMain = CreateWindow("DdeCli16", "DDE Client (WM_DDE_ messages)", WS_OVERLAPPEDWINDOW,
                         20, 20, 560, 230, NULL, NULL, hInst, NULL);
    ShowWindow(hMain, show);
    UpdateWindow(hMain);
    run();
    sprintf(line, "ddecli16 done: %d failure(s)\r\n", failures);
    out(line);
    /* Stay a moment for the screenshot. */
    delay(3000);
    DestroyWindow(hMain);
    while (GetMessage(&msg, NULL, 0, 0))
        DispatchMessage(&msg);
    return failures;
}
