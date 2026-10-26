/* MENU16: a Windows 3.x program with a menu bar from its resources,
 * check marks and grayed items (WM_INITMENUPOPUP), a cascading pop-up,
 * keyboard accelerators (TranslateAccelerator), a pop-up menu on the
 * right button (TrackPopupMenu) and strings from a string table. A test
 * of rs2's native Win16 layer (docs/WIN16.md). */

#include <windows.h>
#include "menu16.h"

static HINSTANCE hInst;
static BOOL grid = TRUE;
static COLORREF colour = RGB(192, 0, 0);
static int size = 40;
static int commands;
static char last[64] = "(none yet)";
static char hint[96];

static void note(HWND hwnd, const char *what)
{
    lstrcpy(last, what);
    commands++;
    InvalidateRect(hwnd, NULL, TRUE);
}

LONG FAR PASCAL _export MenuWndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_INITMENUPOPUP:
        CheckMenuItem((HMENU)wp, IDM_GRID, grid ? MF_CHECKED : MF_UNCHECKED);
        CheckMenuItem((HMENU)wp, IDM_RED, colour == RGB(192, 0, 0) ? MF_CHECKED : MF_UNCHECKED);
        CheckMenuItem((HMENU)wp, IDM_GREEN, colour == RGB(0, 128, 0) ? MF_CHECKED : MF_UNCHECKED);
        CheckMenuItem((HMENU)wp, IDM_BLUE, colour == RGB(0, 0, 192) ? MF_CHECKED : MF_UNCHECKED);
        EnableMenuItem((HMENU)wp, IDM_BIGGER, size < 120 ? MF_ENABLED : MF_GRAYED);
        EnableMenuItem((HMENU)wp, IDM_SMALLER, size > 10 ? MF_ENABLED : MF_GRAYED);
        return 0;
    case WM_COMMAND:
        switch (wp) {
        case IDM_NEW: note(hwnd, "File/New"); break;
        case IDM_OPEN: note(hwnd, "File/Open"); break;
        case IDM_EXIT: DestroyWindow(hwnd); break;
        case IDM_CUT: note(hwnd, "Edit/Cut"); break;
        case IDM_COPY: note(hwnd, "Edit/Copy"); break;
        case IDM_PASTE: note(hwnd, "Edit/Paste"); break;
        case IDM_GRID: grid = !grid; note(hwnd, grid ? "Grid on" : "Grid off"); break;
        case IDM_RED: colour = RGB(192, 0, 0); note(hwnd, "Red"); break;
        case IDM_GREEN: colour = RGB(0, 128, 0); note(hwnd, "Green"); break;
        case IDM_BLUE: colour = RGB(0, 0, 192); note(hwnd, "Blue"); break;
        case IDM_BIGGER: if (size < 120) size += 10; note(hwnd, "Bigger"); break;
        case IDM_SMALLER: if (size > 10) size -= 10; note(hwnd, "Smaller"); break;
        case IDM_ABOUT:
            MessageBox(hwnd, "Menu16\n\nA Windows 3.x program on rs2's own Win16 layer.",
                       "About Menu16", MB_OK | MB_ICONINFORMATION);
            break;
        }
        return 0;
    case WM_RBUTTONUP: {
        /* The Options menu as a pop-up at the pointer. */
        HMENU sub = GetSubMenu(GetMenu(hwnd), 2);
        POINT pt;
        pt.x = LOWORD(lp);
        pt.y = HIWORD(lp);
        ClientToScreen(hwnd, &pt);
        TrackPopupMenu(sub, 0, pt.x, pt.y, 0, hwnd, NULL);
        return 0;
    }
    case WM_PAINT: {
        PAINTSTRUCT ps;
        RECT rc;
        HDC hdc = BeginPaint(hwnd, &ps);
        HBRUSH br;
        char line[96];
        int x, y;
        GetClientRect(hwnd, &rc);
        if (grid) {
            HPEN pen = CreatePen(PS_SOLID, 1, RGB(208, 208, 208));
            HPEN old = SelectObject(hdc, pen);
            for (x = 0; x < rc.right; x += 20) {
                MoveTo(hdc, x, 0);
                LineTo(hdc, x, rc.bottom);
            }
            for (y = 0; y < rc.bottom; y += 20) {
                MoveTo(hdc, 0, y);
                LineTo(hdc, rc.right, y);
            }
            SelectObject(hdc, old);
            DeleteObject(pen);
        }
        br = CreateSolidBrush(colour);
        SelectObject(hdc, br);
        Ellipse(hdc, rc.right / 2 - size, 70, rc.right / 2 + size, 70 + 2 * size);
        SelectObject(hdc, GetStockObject(WHITE_BRUSH));
        DeleteObject(br);
        SetBkMode(hdc, TRANSPARENT);
        TextOut(hdc, 8, 8, hint, lstrlen(hint));
        wsprintf(line, "Last command: %s (%d so far)", (LPSTR)last, commands);
        TextOut(hdc, 8, 30, line, lstrlen(line));
        EndPaint(hwnd, &ps);
        return 0;
    }
    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProc(hwnd, msg, wp, lp);
}

int PASCAL WinMain(HINSTANCE hInstance, HINSTANCE hPrev, LPSTR cmd, int show)
{
    WNDCLASS wc;
    HWND hwnd;
    HACCEL acc;
    MSG msg;
    char title[64];

    hInst = hInstance;
    if (!hPrev) {
        wc.style = CS_HREDRAW | CS_VREDRAW;
        wc.lpfnWndProc = MenuWndProc;
        wc.cbClsExtra = 0;
        wc.cbWndExtra = 0;
        wc.hInstance = hInstance;
        wc.hIcon = LoadIcon(NULL, IDI_APPLICATION);
        wc.hCursor = LoadCursor(NULL, IDC_ARROW);
        wc.hbrBackground = COLOR_WINDOW + 1;
        wc.lpszMenuName = "Menu16";
        wc.lpszClassName = "Menu16";
        if (!RegisterClass(&wc))
            return 0;
    }
    LoadString(hInstance, IDS_TITLE, title, sizeof title);
    LoadString(hInstance, IDS_HINT, hint, sizeof hint);
    hwnd = CreateWindow("Menu16", title, WS_OVERLAPPEDWINDOW,
                        40, 40, 520, 340, NULL, NULL, hInstance, NULL);
    acc = LoadAccelerators(hInstance, "Menu16");
    ShowWindow(hwnd, show);
    UpdateWindow(hwnd);
    while (GetMessage(&msg, NULL, 0, 0)) {
        if (!TranslateAccelerator(hwnd, acc, &msg)) {
            TranslateMessage(&msg);
            DispatchMessage(&msg);
        }
    }
    return msg.wParam;
}
