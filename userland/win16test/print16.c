/* PRINT16: prints a test page with text and graphics on the default
 * printer, as Windows 3.1 programs find and drive it: WIN.INI's
 * [windows] device= entry, CreateDC, GetDeviceCaps, an abort procedure,
 * StartDoc/StartPage/EndPage/EndDoc; then a second document through
 * the Windows 3.0 escapes (STARTDOC, NEWFRAME, ENDDOC). Writes PASS/FAIL
 * lines to standard output and "print16 done: N failure(s)". On rs2 the
 * printer is a spooler queue and the page comes out as PostScript
 * (docs/PRINTING.md, docs/WIN16.md). */

#include <windows.h>
#include <string.h>
#include <stdio.h>
#include <io.h>

static int failures;
static char line[200];
static int abort_calls;

static void out(const char *s)
{
    write(1, s, strlen(s));
}

static void check(int ok, const char *what)
{
    wsprintf(line, "%s %s\r\n", (LPSTR)(ok ? "PASS" : "FAIL"), (LPSTR)what);
    out(line);
    if (!ok)
        failures++;
}

BOOL FAR PASCAL _export AbortProc(HDC hdc, int code)
{
    MSG msg;
    abort_calls++;
    while (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE)) {
        TranslateMessage(&msg);
        DispatchMessage(&msg);
    }
    return TRUE;
}

static HFONT font(HDC dc, int points, int weight, BOOL italic, const char *face)
{
    return CreateFont(-MulDiv(points, GetDeviceCaps(dc, LOGPIXELSY), 72), 0, 0, 0, weight, italic, 0, 0,
                      ANSI_CHARSET, 0, 0, 0, 0, face);
}

/* The test page: a title, text in three faces, a frame, shapes and lines
 * in colours. Units: device pels (300 dpi), an inch margin. */
static void page(HDC dc, const char *title)
{
    int dpi = GetDeviceCaps(dc, LOGPIXELSX), w = GetDeviceCaps(dc, HORZRES), h = GetDeviceCaps(dc, VERTRES);
    int m = dpi, y, i;
    HFONT f, old;
    HPEN pen, oldpen;
    HBRUSH br, oldbr;
    POINT star[5];
    static const char *body[] = {
        "This page was printed by a Windows 3.x program (PRINT16.EXE)",
        "running on rs2's own Win16 layer: GDI calls on a printer DC",
        "become GPI drawing on a PM printer DC, the spooler queues the",
        "job and rs2's PostScript driver writes the page.",
    };

    SetBkMode(dc, TRANSPARENT);
    f = font(dc, 24, FW_BOLD, FALSE, "Times New Roman");
    old = SelectObject(dc, f);
    SetTextColor(dc, RGB(0, 0, 128));
    TextOut(dc, m, m, title, lstrlen(title));
    SelectObject(dc, old);
    DeleteObject(f);

    f = font(dc, 12, FW_NORMAL, FALSE, "Arial");
    old = SelectObject(dc, f);
    SetTextColor(dc, RGB(0, 0, 0));
    y = m + dpi / 2;
    for (i = 0; i < 4; i++) {
        TextOut(dc, m, y, body[i], lstrlen(body[i]));
        y += dpi / 5;
    }
    SelectObject(dc, old);
    DeleteObject(f);

    f = font(dc, 10, FW_NORMAL, TRUE, "Courier New");
    old = SelectObject(dc, f);
    TextOut(dc, m, y + dpi / 10, "Courier New, 10 points, italic: 0123456789", 42);
    SelectObject(dc, old);
    DeleteObject(f);

    /* A frame round the page's content. */
    pen = CreatePen(PS_SOLID, dpi / 50, RGB(0, 0, 0));
    oldpen = SelectObject(dc, pen);
    oldbr = SelectObject(dc, GetStockObject(NULL_BRUSH));
    Rectangle(dc, m / 2, m / 2, w - m / 2, h - m / 2);
    SelectObject(dc, oldpen);
    DeleteObject(pen);

    /* Shapes: a red ellipse, a green box, a blue star, colour lines. */
    y = m + 2 * dpi;
    br = CreateSolidBrush(RGB(255, 0, 0));
    SelectObject(dc, br);
    Ellipse(dc, m, y, m + 2 * dpi, y + dpi);
    SelectObject(dc, oldbr);
    DeleteObject(br);
    br = CreateSolidBrush(RGB(0, 160, 0));
    SelectObject(dc, br);
    Rectangle(dc, m + 5 * dpi / 2, y, m + 4 * dpi, y + dpi);
    SelectObject(dc, oldbr);
    DeleteObject(br);
    for (i = 0; i < 5; i++) {
        static const int sx[5] = {50, 79, 2, 98, 21}, sy[5] = {0, 90, 35, 35, 90};
        star[i].x = m + 9 * dpi / 2 + sx[i] * dpi / 100;
        star[i].y = y + sy[i] * dpi / 100;
    }
    br = CreateSolidBrush(RGB(0, 0, 255));
    SelectObject(dc, br);
    Polygon(dc, star, 5);
    SelectObject(dc, oldbr);
    DeleteObject(br);
    for (i = 0; i < 6; i++) {
        pen = CreatePen(PS_SOLID, 1 + i * 3, RGB(i * 40, 0, 255 - i * 40));
        SelectObject(dc, pen);
        MoveTo(dc, m, y + 3 * dpi / 2 + i * dpi / 6);
        LineTo(dc, w - m, y + 3 * dpi / 2 + i * dpi / 6);
        SelectObject(dc, oldpen);
        DeleteObject(pen);
    }
}

int PASCAL WinMain(HINSTANCE hInst, HINSTANCE hPrev, LPSTR cmd, int show)
{
    char dev[120], *device, *driver, *port;
    HDC dc;
    DOCINFO di;
    FARPROC ap;
    POINT pt;
    int esc, r1, r2, r3;

    out("print16: printing from a Windows program\r\n");
    GetProfileString("windows", "device", "", dev, sizeof dev);
    wsprintf(line, "  [windows] device=%s\r\n", (LPSTR)dev);
    out(line);
    device = strtok(dev, ",");
    driver = strtok(NULL, ",");
    port = strtok(NULL, ",");
    check(device && driver && port, "WIN.INI [windows] device= names the default printer");
    if (!device || !driver || !port)
        goto done;
    dc = CreateDC(driver, device, port, NULL);
    check(dc != 0, "CreateDC on the default printer");
    if (!dc)
        goto done;
    check(GetDeviceCaps(dc, TECHNOLOGY) == DT_RASPRINTER, "GetDeviceCaps TECHNOLOGY: a printer");
    check(GetDeviceCaps(dc, LOGPIXELSX) == 300 && GetDeviceCaps(dc, HORZRES) > 2000, "GetDeviceCaps: 300 dpi, the page in pels");
    esc = NEWFRAME;
    check(Escape(dc, QUERYESCSUPPORT, sizeof esc, (LPSTR)&esc, NULL) > 0, "Escape QUERYESCSUPPORT NEWFRAME");
    check(Escape(dc, GETPHYSPAGESIZE, 0, NULL, &pt) > 0 && pt.x == GetDeviceCaps(dc, HORZRES), "Escape GETPHYSPAGESIZE");

    ap = MakeProcInstance((FARPROC)AbortProc, hInst);
    check(SetAbortProc(dc, (ABORTPROC)ap) > 0, "SetAbortProc");
    di.cbSize = sizeof di;
    di.lpszDocName = "PRINT16 test page";
    di.lpszOutput = NULL;
    r1 = StartDoc(dc, &di);
    r2 = StartPage(dc);
    page(dc, "PRINT16: a page from Windows");
    r3 = EndPage(dc);
    check(r1 > 0 && r2 > 0 && r3 > 0, "StartDoc, StartPage, EndPage");
    check(EndDoc(dc) > 0, "EndDoc: the job is queued");
    check(abort_calls > 0, "the abort procedure was called");

    /* Windows 3.0's way: escapes. */
    r1 = Escape(dc, STARTDOC, 14, "PRINT16 escape", NULL);
    page(dc, "PRINT16: Escape(NEWFRAME)");
    r2 = Escape(dc, NEWFRAME, 0, NULL, NULL);
    r3 = Escape(dc, ENDDOC, 0, NULL, NULL);
    check(r1 > 0 && r2 > 0 && r3 > 0, "Escape STARTDOC, NEWFRAME, ENDDOC");
    FreeProcInstance(ap);
    check(DeleteDC(dc), "DeleteDC");
done:
    sprintf(line, "print16 done: %d failure(s)\r\n", failures);
    out(line);
    return failures;
}
