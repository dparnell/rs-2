/* MDI16: a small multiple document interface program. A frame with an
 * MDI client, text documents (an edit control in each) and pictures
 * (drawn with GDI: a mapping mode, regions, a hatched brush, a memory
 * bitmap), the Window menu (tile, cascade, arrange icons, the list of
 * documents), maximized documents in the frame's menu bar, Ctrl+F4 and
 * Ctrl+F6. A test of rs2's native Win16 layer (docs/WIN16.md).
 *
 * MDI16 DEMO opens two documents of each kind and tiles them; MDI16 MAX
 * opens one maximized; MDI16 ICONS minimizes two and arranges them. */

#include <windows.h>
#include <string.h>
#include "mdi16.h"

static HINSTANCE hInst;
static HWND frame, mdiclient;
static int texts, picts;
static const char *sample = "This is a Windows 3.x MDI document\r\non rs2's native Win16 layer.\r\n\r\n"
                            "Try Window/Tile, Window/Cascade, the\r\nmaximize button and Ctrl+F6.";

static HWND new_doc(BOOL pict, DWORD style)
{
    MDICREATESTRUCT mcs;
    char title[32];

    wsprintf(title, pict ? "Picture %d" : "Text %d", pict ? ++picts : ++texts);
    mcs.szClass = pict ? "Mdi16Pict" : "Mdi16Text";
    mcs.szTitle = title;
    mcs.hOwner = hInst;
    mcs.x = mcs.y = mcs.cx = mcs.cy = CW_USEDEFAULT;
    mcs.style = style;
    mcs.lParam = 0;
    return (HWND)LOWORD(SendMessage(mdiclient, WM_MDICREATE, 0, (LPARAM)(LPMDICREATESTRUCT)&mcs));
}

static void close_all(void)
{
    HWND c;

    while ((c = GetWindow(mdiclient, GW_CHILD)) != NULL)
        SendMessage(mdiclient, WM_MDIDESTROY, (WPARAM)c, 0);
}

LONG FAR PASCAL _export FrameWndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_CREATE: {
        CLIENTCREATESTRUCT ccs;

        ccs.hWindowMenu = GetSubMenu(GetMenu(hwnd), WINDOWMENU);
        ccs.idFirstChild = IDM_FIRSTCHILD;
        mdiclient = CreateWindow("MDICLIENT", NULL, WS_CHILD | WS_CLIPCHILDREN | WS_VISIBLE | WS_VSCROLL | WS_HSCROLL, 0, 0, 0, 0, hwnd,
                                 (HMENU)1, hInst, (LPSTR)&ccs);
        return 0;
    }
    case WM_COMMAND:
        switch (wp) {
        case IDM_NEWTEXT: new_doc(FALSE, 0); return 0;
        case IDM_NEWPICT: new_doc(TRUE, 0); return 0;
        case IDM_CLOSE: {
            HWND a = (HWND)LOWORD(SendMessage(mdiclient, WM_MDIGETACTIVE, 0, 0));
            if (a)
                SendMessage(mdiclient, WM_MDIDESTROY, (WPARAM)a, 0);
            return 0;
        }
        case IDM_EXIT: SendMessage(hwnd, WM_CLOSE, 0, 0); return 0;
        case IDM_TILE: SendMessage(mdiclient, WM_MDITILE, MDITILE_VERTICAL, 0); return 0;
        case IDM_TILEH: SendMessage(mdiclient, WM_MDITILE, MDITILE_HORIZONTAL, 0); return 0;
        case IDM_CASCADE: SendMessage(mdiclient, WM_MDICASCADE, 0, 0); return 0;
        case IDM_ARRANGE: SendMessage(mdiclient, WM_MDIICONARRANGE, 0, 0); return 0;
        case IDM_CLOSEALL: close_all(); return 0;
        case IDM_ABOUT:
            MessageBox(hwnd, "MDI16\n\nA multiple document interface program\non rs2's own Win16 layer.", "About MDI16",
                       MB_OK | MB_ICONINFORMATION);
            return 0;
        }
        break;
    case WM_INITMENUPOPUP: {
        BOOL any = GetWindow(mdiclient, GW_CHILD) != NULL;
        EnableMenuItem((HMENU)wp, IDM_CLOSE, any ? MF_ENABLED : MF_GRAYED);
        EnableMenuItem((HMENU)wp, IDM_CLOSEALL, any ? MF_ENABLED : MF_GRAYED);
        break;
    }
    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    }
    return DefFrameProc(hwnd, mdiclient, msg, wp, lp);
}

LONG FAR PASCAL _export TextWndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    HWND edit = (HWND)GetWindowWord(hwnd, 0);

    switch (msg) {
    case WM_CREATE:
        edit = CreateWindow("EDIT", sample, WS_CHILD | WS_VISIBLE | WS_VSCROLL | ES_MULTILINE | ES_AUTOVSCROLL, 0, 0, 0, 0, hwnd, (HMENU)1,
                            hInst, NULL);
        SetWindowWord(hwnd, 0, (WORD)edit);
        break;
    case WM_SIZE:
        MoveWindow(edit, 0, 0, LOWORD(lp), HIWORD(lp), TRUE);
        break;
    case WM_SETFOCUS:
        SetFocus(edit);
        return 0;
    }
    return DefMDIChildProc(hwnd, msg, wp, lp);
}

/* A picture: a clock face in MM_ISOTROPIC (the logical window is 200 x
 * 200 around the centre, y up), an elliptic region with a hatched brush,
 * and a checkerboard made in a memory bitmap and stretched in. */
static void draw_pict(HWND hwnd, HDC dc)
{
    RECT r;
    int i, w, h;
    HBRUSH hatch, old;
    HRGN rgn;
    HDC mem;
    HBITMAP bm, oldbm;
    static const int sx[12] = { 0, 45, 78, 90, 78, 45, 0, -45, -78, -90, -78, -45 };
    static const int sy[12] = { 90, 78, 45, 0, -45, -78, -90, -78, -45, 0, 45, 78 };

    GetClientRect(hwnd, &r);
    w = r.right;
    h = r.bottom;
    /* The checkerboard in the top-left corner. */
    mem = CreateCompatibleDC(dc);
    bm = CreateCompatibleBitmap(dc, 8, 8);
    oldbm = SelectObject(mem, bm);
    PatBlt(mem, 0, 0, 8, 8, WHITENESS);
    for (i = 0; i < 8; i++)
        PatBlt(mem, (i & 1) * 4, i, 4, 1, BLACKNESS);
    StretchBlt(dc, 4, 4, 32, 32, mem, 0, 0, 8, 8, SRCCOPY);
    SelectObject(mem, oldbm);
    DeleteObject(bm);
    DeleteDC(mem);
    /* A region: an ellipse less a rectangle, hatched. */
    rgn = CreateEllipticRgn(w / 2 - w / 3, h / 2 - h / 3, w / 2 + w / 3, h / 2 + h / 3);
    {
        HRGN cut = CreateRectRgn(w / 2 - 10, 0, w / 2 + 10, h);
        CombineRgn(rgn, rgn, cut, RGN_DIFF);
        DeleteObject(cut);
    }
    hatch = CreateHatchBrush(HS_DIAGCROSS, RGB(0, 128, 255));
    FillRgn(dc, rgn, hatch);
    FrameRgn(dc, rgn, GetStockObject(BLACK_BRUSH), 1, 1);
    DeleteObject(hatch);
    DeleteObject(rgn);
    /* The clock face. */
    SaveDC(dc);
    SetMapMode(dc, MM_ISOTROPIC);
    SetWindowExt(dc, 100, 100);
    SetViewportExt(dc, w / 2, -h / 2);
    SetViewportOrg(dc, w / 2, h / 2);
    old = SelectObject(dc, GetStockObject(BLACK_BRUSH));
    for (i = 0; i < 12; i++)
        Ellipse(dc, sx[i] - 4, sy[i] + 4, sx[i] + 4, sy[i] - 4);
    SelectObject(dc, CreatePen(PS_SOLID, 3, RGB(192, 0, 0)));
    MoveTo(dc, 0, 0);
    LineTo(dc, 0, 70);
    MoveTo(dc, 0, 0);
    LineTo(dc, 50, -20);
    DeleteObject(SelectObject(dc, GetStockObject(BLACK_PEN)));
    SelectObject(dc, old);
    RestoreDC(dc, -1);
}

LONG FAR PASCAL _export PictWndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    if (msg == WM_PAINT) {
        PAINTSTRUCT ps;
        BeginPaint(hwnd, &ps);
        draw_pict(hwnd, ps.hdc);
        EndPaint(hwnd, &ps);
        return 0;
    }
    if (msg == WM_SIZE)
        InvalidateRect(hwnd, NULL, TRUE);
    return DefMDIChildProc(hwnd, msg, wp, lp);
}

int PASCAL WinMain(HINSTANCE inst, HINSTANCE prev, LPSTR cmd, int show)
{
    WNDCLASS wc;
    MSG msg;
    HACCEL acc;

    hInst = inst;
    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = FrameWndProc;
    wc.hInstance = inst;
    wc.hIcon = LoadIcon(NULL, IDI_APPLICATION);
    wc.hCursor = LoadCursor(NULL, IDC_ARROW);
    wc.hbrBackground = (HBRUSH)(COLOR_APPWORKSPACE + 1);
    wc.lpszMenuName = "Mdi16";
    wc.lpszClassName = "Mdi16Frame";
    RegisterClass(&wc);
    wc.lpfnWndProc = TextWndProc;
    wc.cbWndExtra = 2;
    wc.hbrBackground = (HBRUSH)(COLOR_WINDOW + 1);
    wc.lpszMenuName = NULL;
    wc.lpszClassName = "Mdi16Text";
    RegisterClass(&wc);
    wc.lpfnWndProc = PictWndProc;
    wc.cbWndExtra = 0;
    wc.lpszClassName = "Mdi16Pict";
    wc.style = CS_HREDRAW | CS_VREDRAW;
    RegisterClass(&wc);
    frame = CreateWindow("Mdi16Frame", "MDI16", WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN, 40, 30, 700, 500, NULL, NULL, inst, NULL);
    ShowWindow(frame, show);
    UpdateWindow(frame);
    acc = LoadAccelerators(inst, "Mdi16");
    if (lstrcmpi(cmd, "MAX") == 0) {
        new_doc(FALSE, WS_MAXIMIZE);
        new_doc(TRUE, 0);
    } else if (lstrcmpi(cmd, "ICONS") == 0) {
        ShowWindow(new_doc(FALSE, 0), SW_MINIMIZE);
        ShowWindow(new_doc(TRUE, 0), SW_MINIMIZE);
        new_doc(TRUE, 0);
        SendMessage(mdiclient, WM_MDIICONARRANGE, 0, 0);
        SendMessage(mdiclient, WM_MDITILE, MDITILE_VERTICAL, 0);
    } else {
        new_doc(FALSE, 0);
        new_doc(TRUE, 0);
        if (lstrcmpi(cmd, "DEMO") == 0) {
            new_doc(FALSE, 0);
            new_doc(TRUE, 0);
            SendMessage(mdiclient, WM_MDITILE, MDITILE_VERTICAL, 0);
        }
    }
    while (GetMessage(&msg, NULL, 0, 0)) {
        if (!TranslateMDISysAccel(mdiclient, &msg) && !TranslateAccelerator(frame, acc, &msg)) {
            TranslateMessage(&msg);
            DispatchMessage(&msg);
        }
    }
    return msg.wParam;
}
