/* CPMHELLO.EXE: a 64-bit Presentation Manager program in C (docs/API64.md):
 * a frame window with a menu from its resources (pmhello.rc) that draws
 * with GPI. /SELFTEST checks it and closes it after a few timer ticks,
 * printing PASS/FAIL and "cpmhello selftest done: N failure(s)". */

#include <os2.h>

#define ID_MAIN 1
#define IDM_EXIT 12
#define IDS_TITLE 20

static int selftest, failures, painted, ticks, created;

static void out(const char *s) {
    ULONG n, len = 0;
    while (s[len])
        len++;
    DosWrite(1, s, len, &n);
}

static void check(const char *what, int ok) {
    if (!selftest)
        return;
    out(ok ? "PASS: " : "FAIL: ");
    out(what);
    out("\r\n");
    if (!ok)
        failures++;
}

static MRESULT EXPENTRY client(HWND hwnd, ULONG msg, MPARAM mp1, MPARAM mp2) {
    switch (msg) {
    case WM_CREATE:
        created = ((PCREATESTRUCT)mp2)->hwndParent != NULLHANDLE;
        break;
    case WM_PAINT: {
        RECTL r;
        POINTL p;
        HPS hps = WinBeginPaint(hwnd, NULLHANDLE, NULL);
        WinQueryWindowRect(hwnd, &r);
        WinFillRect(hps, &r, CLR_PALEGRAY);
        GpiSetColor(hps, CLR_DARKBLUE);
        p.x = 20;
        p.y = r.yTop - 30;
        static const char text[] = "A 64-bit PM program written in C";
        GpiCharStringAt(hps, &p, sizeof text - 1, (PCH)text);
        for (int i = 0; i < 4; i++) {
            GpiSetColor(hps, CLR_RED + i);
            p.x = 20 + i * 60;
            p.y = 20;
            GpiMove(hps, &p);
            p.x += 50;
            p.y += 50;
            GpiBox(hps, DRO_OUTLINEFILL, &p, 20, 20);
        }
        WinEndPaint(hps);
        painted++;
        return 0;
    }
    case WM_TIMER:
        if (selftest && ++ticks >= 3 && painted)
            WinPostMsg(hwnd, WM_CLOSE, MPVOID, MPVOID);
        return 0;
    case WM_COMMAND:
        if (SHORT1FROMMP(mp1) == IDM_EXIT)
            WinPostMsg(hwnd, WM_CLOSE, MPVOID, MPVOID);
        return 0;
    }
    return WinDefWindowProc(hwnd, msg, mp1, mp2);
}

int main(int argc, char **argv) {
    for (int i = 1; i < argc; i++) {
        const char *a = argv[i];
        selftest |= a[0] == '/' && (a[1] == 's' || a[1] == 'S');
    }
    HAB hab = WinInitialize(0);
    HMQ hmq = WinCreateMsgQueue(hab, 0);
    check("WinInitialize, WinCreateMsgQueue", hab && hmq);
    check("WinRegisterClass", WinRegisterClass(hab, "CPmHello", client, CS_SIZEREDRAW, 0));
    char title[64];
    LONG n = WinLoadString(hab, NULLHANDLE, IDS_TITLE, sizeof title, title);
    check("WinLoadString (string table)", n > 0 && title[0] == 'C');
    ULONG fl = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST | FCF_MENU;
    HWND hwndClient;
    HWND frame = WinCreateStdWindow(HWND_DESKTOP, 0, &fl, "CPmHello", title, 0, NULLHANDLE, ID_MAIN, &hwndClient);
    check("WinCreateStdWindow", frame && hwndClient);
    check("WM_CREATE's CREATESTRUCT", created);
    check("menu resource", WinWindowFromID(frame, FID_MENU) != NULLHANDLE);
    WinSetWindowPos(frame, HWND_TOP, 100, 120, 420, 220, SWP_SIZE | SWP_MOVE | SWP_SHOW | SWP_ACTIVATE | SWP_ZORDER);
    WinStartTimer(hab, hwndClient, 1, 300);
    QMSG q;
    while (WinGetMsg(hab, &q, NULLHANDLE, 0, 0))
        WinDispatchMsg(hab, &q);
    check("message loop, painting, timer", q.msg == WM_QUIT && painted > 0 && (ticks >= 3 || !selftest));
    WinDestroyWindow(frame);
    WinDestroyMsgQueue(hmq);
    WinTerminate(hab);
    if (selftest) {
        char line[64] = "cpmhello selftest done: ";
        char *e = line + 24;
        *e++ = '0' + failures;
        const char *t = " failure(s)\r\n";
        while ((*e++ = *t++))
            ;
        out(line);
    }
    return failures;
}
