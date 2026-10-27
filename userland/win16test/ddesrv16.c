/* DDESRV16: a DDE server written with DDEML. Service "RS2DDE", topic
 * "Clock", items "Time" (the time, hot-linked: advised every second),
 * "Counter" (advised when it changes) and "Text" (any text poked to
 * it); execute "[inc]" adds one to the counter, "[set n]" sets it,
 * "[quit]" ends the server. Its window lists what clients did. With
 * DDECLI16 (raw WM_DDE_* messages) a test of rs2's DDE between Windows
 * tasks (docs/WIN16.md). */

#include <windows.h>
#include <ddeml.h>
#include <string.h>
#include <stdlib.h>
#include <time.h>

#pragma library("ddeml")

static HINSTANCE hInst;
static HWND hMain;
static DWORD inst;
static HSZ hszService, hszTopic, hszTime, hszCounter, hszText;
static long counter;
static char text[128] = "(nothing yet)";
static char log[12][80];
static int nlog;

static void note(const char *s)
{
    int i;
    if (nlog == 12) {
        for (i = 1; i < 12; i++)
            lstrcpy(log[i - 1], log[i]);
        nlog = 11;
    }
    lstrcpy(log[nlog++], s);
    if (hMain)
        InvalidateRect(hMain, NULL, TRUE);
}

static void now(char *t)
{
    time_t n = time(NULL);
    struct tm *tm = localtime(&n);
    wsprintf(t, "%02d:%02d:%02d", tm->tm_hour, tm->tm_min, tm->tm_sec);
}

/* The data of an item, as text (CF_TEXT). */
static HDDEDATA item_data(HSZ item)
{
    char t[140];
    if (DdeCmpStringHandles(item, hszTime) == 0)
        now(t);
    else if (DdeCmpStringHandles(item, hszCounter) == 0)
        wsprintf(t, "%ld", counter);
    else if (DdeCmpStringHandles(item, hszText) == 0)
        lstrcpy(t, text);
    else
        return 0;
    return DdeCreateDataHandle(inst, t, lstrlen(t) + 1, 0, item, CF_TEXT, 0);
}

HDDEDATA CALLBACK _export DdeCallback(UINT type, UINT fmt, HCONV conv, HSZ hsz1, HSZ hsz2,
                                      HDDEDATA data, DWORD d1, DWORD d2)
{
    char t[160], item[40];
    switch (type) {
    case XTYP_CONNECT:
        /* hsz1 is the topic. */
        return (HDDEDATA)(DdeCmpStringHandles(hsz1, hszTopic) == 0);
    case XTYP_CONNECT_CONFIRM:
        note("client connected");
        return 0;
    case XTYP_DISCONNECT:
        note("client disconnected");
        return 0;
    case XTYP_REQUEST:
    case XTYP_ADVREQ:
        if (fmt != CF_TEXT)
            return 0;
        DdeQueryString(inst, hsz2, item, sizeof item, CP_WINANSI);
        if (type == XTYP_REQUEST) {
            wsprintf(t, "request %s", (LPSTR)item);
            note(t);
        }
        return item_data(hsz2);
    case XTYP_POKE:
        if (DdeCmpStringHandles(hsz2, hszText) != 0 || fmt != CF_TEXT)
            return DDE_FNOTPROCESSED;
        DdeGetData(data, text, sizeof text - 1, 0);
        text[sizeof text - 1] = 0;
        wsprintf(t, "poke Text: %s", (LPSTR)text);
        note(t);
        DdePostAdvise(inst, hszTopic, hszText);
        return (HDDEDATA)DDE_FACK;
    case XTYP_EXECUTE: {
        char cmd[80];
        DdeGetData(data, cmd, sizeof cmd - 1, 0);
        cmd[sizeof cmd - 1] = 0;
        wsprintf(t, "execute %s", (LPSTR)cmd);
        note(t);
        if (lstrcmpi(cmd, "[inc]") == 0)
            counter++;
        else if (_fstrnicmp(cmd, "[set ", 5) == 0)
            counter = atol(cmd + 5);
        else if (lstrcmpi(cmd, "[quit]") == 0)
            PostMessage(hMain, WM_CLOSE, 0, 0);
        else
            return DDE_FNOTPROCESSED;
        DdePostAdvise(inst, hszTopic, hszCounter);
        return (HDDEDATA)DDE_FACK;
    }
    case XTYP_ADVSTART:
        DdeQueryString(inst, hsz2, item, sizeof item, CP_WINANSI);
        wsprintf(t, "advise %s", (LPSTR)item);
        note(t);
        return (HDDEDATA)(fmt == CF_TEXT && (DdeCmpStringHandles(hsz2, hszTime) == 0 ||
                          DdeCmpStringHandles(hsz2, hszCounter) == 0 || DdeCmpStringHandles(hsz2, hszText) == 0));
    case XTYP_ADVSTOP:
        DdeQueryString(inst, hsz2, item, sizeof item, CP_WINANSI);
        wsprintf(t, "unadvise %s", (LPSTR)item);
        note(t);
        return 0;
    }
    return 0;
}

LONG FAR PASCAL _export SrvWndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_TIMER:
        DdePostAdvise(inst, hszTopic, hszTime);
        return 0;
    case WM_PAINT: {
        PAINTSTRUCT ps;
        HDC dc = BeginPaint(hwnd, &ps);
        char t[160];
        int i;
        TEXTMETRIC tm;
        GetTextMetrics(dc, &tm);
        wsprintf(t, "DDE server RS2DDE|Clock: counter %ld, text \"%s\"", counter, (LPSTR)text);
        TextOut(dc, 8, 6, t, lstrlen(t));
        for (i = 0; i < nlog; i++)
            TextOut(dc, 8, 10 + (i + 1) * tm.tmHeight, log[i], lstrlen(log[i]));
        EndPaint(hwnd, &ps);
        return 0;
    }
    case WM_DESTROY:
        KillTimer(hwnd, 1);
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProc(hwnd, msg, wp, lp);
}

int PASCAL WinMain(HINSTANCE hInstance, HINSTANCE hPrev, LPSTR cmd, int show)
{
    WNDCLASS wc;
    MSG msg;
    FARPROC cb;

    hInst = hInstance;
    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = SrvWndProc;
    wc.hInstance = hInstance;
    wc.hIcon = LoadIcon(NULL, IDI_APPLICATION);
    wc.hCursor = LoadCursor(NULL, IDC_ARROW);
    wc.hbrBackground = COLOR_WINDOW + 1;
    wc.lpszClassName = "DdeSrv16";
    RegisterClass(&wc);
    hMain = CreateWindow("DdeSrv16", "DDE Server (DDEML)", WS_OVERLAPPEDWINDOW,
                         20, 260, 460, 230, NULL, NULL, hInstance, NULL);
    cb = MakeProcInstance((FARPROC)DdeCallback, hInstance);
    if (DdeInitialize(&inst, (PFNCALLBACK)cb, APPCLASS_STANDARD | CBF_SKIP_REGISTRATIONS | CBF_SKIP_UNREGISTRATIONS, 0)) {
        MessageBox(NULL, "DdeInitialize failed", "DDESRV16", MB_OK);
        return 1;
    }
    hszService = DdeCreateStringHandle(inst, "RS2DDE", CP_WINANSI);
    hszTopic = DdeCreateStringHandle(inst, "Clock", CP_WINANSI);
    hszTime = DdeCreateStringHandle(inst, "Time", CP_WINANSI);
    hszCounter = DdeCreateStringHandle(inst, "Counter", CP_WINANSI);
    hszText = DdeCreateStringHandle(inst, "Text", CP_WINANSI);
    DdeNameService(inst, hszService, 0, DNS_REGISTER);
    note("serving RS2DDE|Clock");
    SetTimer(hMain, 1, 1000, NULL);
    ShowWindow(hMain, show);
    UpdateWindow(hMain);
    while (GetMessage(&msg, NULL, 0, 0)) {
        TranslateMessage(&msg);
        DispatchMessage(&msg);
    }
    DdeNameService(inst, hszService, 0, DNS_UNREGISTER);
    DdeFreeStringHandle(inst, hszService);
    DdeFreeStringHandle(inst, hszTopic);
    DdeFreeStringHandle(inst, hszTime);
    DdeFreeStringHandle(inst, hszCounter);
    DdeFreeStringHandle(inst, hszText);
    DdeUninitialize(inst);
    return msg.wParam;
}
