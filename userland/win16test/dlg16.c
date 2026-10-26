/* DLG16: a modal dialog with one of each of Windows' controls, from a
 * dialog template: edit controls (plain, password, numeric, multi-line),
 * radio buttons in a group box, check boxes (2 and 3 states), combo boxes
 * (simple, drop-down, drop-down list), a multiple-selection list box, a
 * scroll bar, statics, push buttons. The dialog manager's keyboard
 * interface (Tab, arrows, mnemonics, Enter, Esc) and the controls'
 * messages and notifications. A test of rs2's native Win16 layer
 * (docs/WIN16.md). */

#include <windows.h>
#include "dlg16.h"

static HINSTANCE hInst;
static char summary[512] = "Choose Dialog/Order (it opens by itself at the start).";

static const char *colours[] = { "Red", "Green", "Blue", "Yellow", "Black", "White" };
static const char *fonts[] = { "Helv", "Courier", "Tms Rmn", "System", "Arial" };
static const char *toppings[] = { "Ham", "Mushrooms", "Pineapple", "Salami", "Peppers",
                                  "Onions", "Anchovies", "Spinach", "Tomato" };
static const char *drinks[] = { "Water", "Cola", "Juice", "Coffee", "Tea" };

static void reset(HWND dlg)
{
    int i;
    SetDlgItemText(dlg, IDC_NAME, "rs2");
    SetDlgItemText(dlg, IDC_PASS, "secret");
    SetDlgItemInt(dlg, IDC_AGE, 30, FALSE);
    CheckRadioButton(dlg, IDC_SMALL, IDC_LARGE, IDC_MEDIUM);
    CheckDlgButton(dlg, IDC_CHEESE, 1);
    CheckDlgButton(dlg, IDC_OLIVES, 2);
    SendDlgItemMessage(dlg, IDC_COLOUR, CB_SETCURSEL, 0, 0);
    SendDlgItemMessage(dlg, IDC_FONT, CB_SETCURSEL, 0, 0);
    SendDlgItemMessage(dlg, IDC_DRINK, CB_SETCURSEL, 1, 0);
    SendDlgItemMessage(dlg, IDC_TOPPINGS, LB_SETSEL, FALSE, -1L);
    SendDlgItemMessage(dlg, IDC_TOPPINGS, LB_SETSEL, TRUE, 1L);
    SetScrollRange(GetDlgItem(dlg, IDC_VOLUME), SB_CTL, 0, 100, FALSE);
    SetScrollPos(GetDlgItem(dlg, IDC_VOLUME), SB_CTL, 50, TRUE);
    SetDlgItemInt(dlg, IDC_VOLVAL, 50, FALSE);
    SetDlgItemText(dlg, IDC_NOTES, "Ring the bell twice.\r\nLeave it at the door.");
    (void)i;
}

static void collect(HWND dlg)
{
    char name[40], colour[20], font[20], drink[20], top[20], notes[200];
    int age, size, cheese, olives, n, i, vol;
    BOOL ok;
    int sel[16];
    char *p;

    GetDlgItemText(dlg, IDC_NAME, name, sizeof name);
    age = GetDlgItemInt(dlg, IDC_AGE, &ok, FALSE);
    size = IsDlgButtonChecked(dlg, IDC_SMALL) ? 1 : IsDlgButtonChecked(dlg, IDC_MEDIUM) ? 2 : 3;
    cheese = IsDlgButtonChecked(dlg, IDC_CHEESE);
    olives = IsDlgButtonChecked(dlg, IDC_OLIVES);
    SendDlgItemMessage(dlg, IDC_COLOUR, CB_GETLBTEXT,
                       (WPARAM)SendDlgItemMessage(dlg, IDC_COLOUR, CB_GETCURSEL, 0, 0), (LPARAM)(LPSTR)colour);
    GetDlgItemText(dlg, IDC_FONT, font, sizeof font);
    GetDlgItemText(dlg, IDC_DRINK, drink, sizeof drink);
    GetDlgItemText(dlg, IDC_NOTES, notes, sizeof notes);
    vol = GetScrollPos(GetDlgItem(dlg, IDC_VOLUME), SB_CTL);
    p = summary;
    p += wsprintf(p, "%s (age %d%s), size %s, cheese %s, olives %s,\n",
                  (LPSTR)name, age, (LPSTR)(ok ? "" : "?"), (LPSTR)(size == 1 ? "small" : size == 2 ? "medium" : "large"),
                  (LPSTR)(cheese ? "yes" : "no"), (LPSTR)(olives == 2 ? "maybe" : olives ? "yes" : "no"));
    p += wsprintf(p, "colour %s, font %s, drink %s, volume %d,\ntoppings:", (LPSTR)colour, (LPSTR)font, (LPSTR)drink, vol);
    n = (int)SendDlgItemMessage(dlg, IDC_TOPPINGS, LB_GETSELITEMS, 16, (LPARAM)(int FAR *)sel);
    for (i = 0; i < n; i++) {
        SendDlgItemMessage(dlg, IDC_TOPPINGS, LB_GETTEXT, sel[i], (LPARAM)(LPSTR)top);
        p += wsprintf(p, " %s", (LPSTR)top);
    }
    wsprintf(p, "\nnotes: %s", (LPSTR)notes);
}

BOOL FAR PASCAL _export OrderDlgProc(HWND dlg, UINT msg, WPARAM wp, LPARAM lp)
{
    int i, pos;
    HWND sb;

    switch (msg) {
    case WM_INITDIALOG:
        for (i = 0; i < 6; i++)
            SendDlgItemMessage(dlg, IDC_COLOUR, CB_ADDSTRING, 0, (LPARAM)(LPSTR)colours[i]);
        for (i = 0; i < 5; i++)
            SendDlgItemMessage(dlg, IDC_FONT, CB_ADDSTRING, 0, (LPARAM)(LPSTR)fonts[i]);
        for (i = 0; i < 9; i++)
            SendDlgItemMessage(dlg, IDC_TOPPINGS, LB_ADDSTRING, 0, (LPARAM)(LPSTR)toppings[i]);
        for (i = 0; i < 5; i++)
            SendDlgItemMessage(dlg, IDC_DRINK, CB_ADDSTRING, 0, (LPARAM)(LPSTR)drinks[i]);
        SendDlgItemMessage(dlg, IDC_NAME, EM_LIMITTEXT, 30, 0);
        reset(dlg);
        return TRUE;
    case WM_HSCROLL:
        sb = (HWND)HIWORD(lp);
        pos = GetScrollPos(sb, SB_CTL);
        switch (wp) {
        case SB_LINEUP: pos -= 1; break;
        case SB_LINEDOWN: pos += 1; break;
        case SB_PAGEUP: pos -= 10; break;
        case SB_PAGEDOWN: pos += 10; break;
        case SB_THUMBTRACK:
        case SB_THUMBPOSITION: pos = LOWORD(lp); break;
        case SB_TOP: pos = 0; break;
        case SB_BOTTOM: pos = 100; break;
        }
        if (pos < 0) pos = 0;
        if (pos > 100) pos = 100;
        SetScrollPos(sb, SB_CTL, pos, TRUE);
        SetDlgItemInt(dlg, IDC_VOLVAL, pos, FALSE);
        return TRUE;
    case WM_COMMAND:
        switch (wp) {
        case IDOK: {
            BOOL ok;
            GetDlgItemInt(dlg, IDC_AGE, &ok, FALSE);
            if (!ok) {
                MessageBox(dlg, "The age must be a number.", "Order", MB_OK | MB_ICONEXCLAMATION);
                SetFocus(GetDlgItem(dlg, IDC_AGE));
                return TRUE;
            }
            collect(dlg);
            EndDialog(dlg, TRUE);
            return TRUE;
        }
        case IDCANCEL:
            EndDialog(dlg, FALSE);
            return TRUE;
        case IDC_RESET:
            reset(dlg);
            return TRUE;
        }
        break;
    }
    return FALSE;
}

LONG FAR PASCAL _export MainWndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_COMMAND:
        if (wp == IDM_DIALOG) {
            FARPROC proc = MakeProcInstance((FARPROC)OrderDlgProc, hInst);
            int r = DialogBox(hInst, "Order", hwnd, (DLGPROC)proc);
            FreeProcInstance(proc);
            if (!r)
                lstrcpy(summary, "Cancelled.");
            InvalidateRect(hwnd, NULL, TRUE);
        } else if (wp == IDM_EXIT) {
            DestroyWindow(hwnd);
        }
        return 0;
    case WM_PAINT: {
        PAINTSTRUCT ps;
        RECT rc;
        HDC hdc = BeginPaint(hwnd, &ps);
        GetClientRect(hwnd, &rc);
        InflateRect(&rc, -8, -8);
        DrawText(hdc, summary, -1, &rc, DT_LEFT | DT_WORDBREAK);
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
    MSG msg;

    hInst = hInstance;
    if (!hPrev) {
        wc.style = CS_HREDRAW | CS_VREDRAW;
        wc.lpfnWndProc = MainWndProc;
        wc.cbClsExtra = 0;
        wc.cbWndExtra = 0;
        wc.hInstance = hInstance;
        wc.hIcon = LoadIcon(NULL, IDI_APPLICATION);
        wc.hCursor = LoadCursor(NULL, IDC_ARROW);
        wc.hbrBackground = COLOR_WINDOW + 1;
        wc.lpszMenuName = "Dlg16";
        wc.lpszClassName = "Dlg16";
        if (!RegisterClass(&wc))
            return 0;
    }
    hwnd = CreateWindow("Dlg16", "Dlg16: dialogs and controls", WS_OVERLAPPEDWINDOW,
                        20, 20, 600, 200, NULL, NULL, hInstance, NULL);
    ShowWindow(hwnd, show);
    UpdateWindow(hwnd);
    PostMessage(hwnd, WM_COMMAND, IDM_DIALOG, 0);
    while (GetMessage(&msg, NULL, 0, 0)) {
        TranslateMessage(&msg);
        DispatchMessage(&msg);
    }
    return msg.wParam;
}
