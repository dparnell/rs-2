/* NOTE16: a small Notepad: a multi-line edit control filling the
 * window, with scroll bars, the clipboard (Edit menu and Ctrl+X/C/V),
 * undo, word wrap, files read and written with _lopen/_lread/_lwrite
 * (a dialog asks for the name), and a check for unsaved changes. A test
 * of rs2's native Win16 layer (docs/WIN16.md). */

#include <windows.h>
#include <time.h>
#include "note16.h"

static HINSTANCE hInst;
static HWND hEdit;
static BOOL wrap;
static char file[128] = "C:\\NOTE16.TXT";

static const char welcome[] =
    "Note16: a Windows 3.x Notepad on rs2's own Win16 layer.\r\n"
    "\r\n"
    "Everything here is drawn by WIN16.DLL: the menu bar is PM's,\r\n"
    "the edit control, its scroll bars and the caret are Windows-style\r\n"
    "controls in Rust, drawn through WIN16's GDI.\r\n"
    "\r\n"
    "Try: typing, Shift+arrows, Ctrl+C/X/V (PM's clipboard), Ctrl+Z,\r\n"
    "File/Save (C:\\NOTE16.TXT), Options/Word Wrap, F5 for the time.\r\n"
    "\r\n"
    "A long line to scroll sideways: 0123456789 abcdefghijklmnopqrstuvwxyz ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789 abcdefghijklmnopqrstuvwxyz\r\n";

static void set_title(HWND hwnd)
{
    char t[160];
    wsprintf(t, "Note16 - %s", (LPSTR)file);
    SetWindowText(hwnd, t);
}

static HWND make_edit(HWND hwnd)
{
    DWORD style = WS_CHILD | WS_VISIBLE | WS_VSCROLL | ES_MULTILINE | ES_AUTOVSCROLL | ES_NOHIDESEL;
    RECT rc;
    HWND e;
    if (!wrap)
        style |= WS_HSCROLL | ES_AUTOHSCROLL;
    GetClientRect(hwnd, &rc);
    e = CreateWindow("EDIT", "", style, 0, 0, rc.right, rc.bottom, hwnd, (HMENU)1, hInst, NULL);
    SendMessage(e, WM_SETFONT, (WPARAM)GetStockObject(SYSTEM_FIXED_FONT), 0);
    SendMessage(e, EM_LIMITTEXT, 32000, 0);
    return e;
}

BOOL FAR PASCAL _export FileDlgProc(HWND dlg, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_INITDIALOG:
        SetDlgItemText(dlg, IDC_FILE, file);
        return TRUE;
    case WM_COMMAND:
        if (wp == IDOK) {
            GetDlgItemText(dlg, IDC_FILE, file, sizeof file);
            EndDialog(dlg, TRUE);
            return TRUE;
        }
        if (wp == IDCANCEL) {
            EndDialog(dlg, FALSE);
            return TRUE;
        }
        break;
    }
    return FALSE;
}

static BOOL ask_name(HWND hwnd)
{
    FARPROC p = MakeProcInstance((FARPROC)FileDlgProc, hInst);
    BOOL r = DialogBox(hInst, "FileName", hwnd, (DLGPROC)p);
    FreeProcInstance(p);
    return r;
}

static void load(HWND hwnd)
{
    HFILE f = _lopen(file, OF_READ);
    HGLOBAL h;
    char FAR *buf;
    UINT n;
    if (f == HFILE_ERROR) {
        MessageBox(hwnd, "Cannot open the file.", "Note16", MB_OK | MB_ICONEXCLAMATION);
        return;
    }
    h = GlobalAlloc(GMEM_MOVEABLE, 32001L);
    buf = GlobalLock(h);
    n = _lread(f, buf, 32000);
    _lclose(f);
    if (n == (UINT)-1)
        n = 0;
    buf[n] = 0;
    SetWindowText(hEdit, buf);
    GlobalUnlock(h);
    GlobalFree(h);
    SendMessage(hEdit, EM_SETMODIFY, FALSE, 0);
    set_title(hwnd);
}

static BOOL save(HWND hwnd)
{
    int n = GetWindowTextLength(hEdit);
    HGLOBAL h = GlobalAlloc(GMEM_MOVEABLE, (DWORD)n + 1);
    char FAR *buf = GlobalLock(h);
    HFILE f;
    BOOL ok;
    GetWindowText(hEdit, buf, n + 1);
    f = _lcreat(file, 0);
    ok = f != HFILE_ERROR && _lwrite(f, buf, n) == (UINT)n;
    if (f != HFILE_ERROR)
        _lclose(f);
    GlobalUnlock(h);
    GlobalFree(h);
    if (!ok) {
        MessageBox(hwnd, "Cannot write the file.", "Note16", MB_OK | MB_ICONEXCLAMATION);
        return FALSE;
    }
    SendMessage(hEdit, EM_SETMODIFY, FALSE, 0);
    set_title(hwnd);
    return TRUE;
}

/* Unsaved changes: save them? FALSE to stay. */
static BOOL may_close(HWND hwnd)
{
    if (!SendMessage(hEdit, EM_GETMODIFY, 0, 0))
        return TRUE;
    switch (MessageBox(hwnd, "The text has changed. Save the changes?", "Note16",
                       MB_YESNOCANCEL | MB_ICONQUESTION)) {
    case IDYES: return save(hwnd);
    case IDNO: return TRUE;
    }
    return FALSE;
}

LONG FAR PASCAL _export NoteWndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    DWORD sel;
    switch (msg) {
    case WM_CREATE:
        hEdit = make_edit(hwnd);
        SetWindowText(hEdit, welcome);
        SendMessage(hEdit, EM_SETMODIFY, FALSE, 0);
        return 0;
    case WM_SIZE:
        MoveWindow(hEdit, 0, 0, LOWORD(lp), HIWORD(lp), TRUE);
        return 0;
    case WM_SETFOCUS:
        SetFocus(hEdit);
        return 0;
    case WM_INITMENUPOPUP:
        sel = SendMessage(hEdit, EM_GETSEL, 0, 0);
        EnableMenuItem((HMENU)wp, IDM_UNDO, SendMessage(hEdit, EM_CANUNDO, 0, 0) ? MF_ENABLED : MF_GRAYED);
        EnableMenuItem((HMENU)wp, IDM_CUT, HIWORD(sel) != LOWORD(sel) ? MF_ENABLED : MF_GRAYED);
        EnableMenuItem((HMENU)wp, IDM_COPY, HIWORD(sel) != LOWORD(sel) ? MF_ENABLED : MF_GRAYED);
        EnableMenuItem((HMENU)wp, IDM_DELETE, HIWORD(sel) != LOWORD(sel) ? MF_ENABLED : MF_GRAYED);
        EnableMenuItem((HMENU)wp, IDM_PASTE, IsClipboardFormatAvailable(CF_TEXT) ? MF_ENABLED : MF_GRAYED);
        CheckMenuItem((HMENU)wp, IDM_WRAP, wrap ? MF_CHECKED : MF_UNCHECKED);
        return 0;
    case WM_COMMAND:
        if (wp == 1) {
            /* From the edit control: EN_ERRSPACE, EN_MAXTEXT. */
            if (HIWORD(lp) == EN_MAXTEXT)
                MessageBeep(0);
            return 0;
        }
        switch (wp) {
        case IDM_NEW:
            if (may_close(hwnd)) {
                SetWindowText(hEdit, "");
                lstrcpy(file, "C:\\UNTITLED.TXT");
                set_title(hwnd);
            }
            break;
        case IDM_OPEN:
            if (may_close(hwnd) && ask_name(hwnd))
                load(hwnd);
            break;
        case IDM_SAVE: save(hwnd); break;
        case IDM_SAVEAS: if (ask_name(hwnd)) save(hwnd); break;
        case IDM_EXIT: SendMessage(hwnd, WM_CLOSE, 0, 0); break;
        case IDM_UNDO: SendMessage(hEdit, WM_UNDO, 0, 0); break;
        case IDM_CUT: SendMessage(hEdit, WM_CUT, 0, 0); break;
        case IDM_COPY: SendMessage(hEdit, WM_COPY, 0, 0); break;
        case IDM_PASTE: SendMessage(hEdit, WM_PASTE, 0, 0); break;
        case IDM_DELETE: SendMessage(hEdit, WM_CLEAR, 0, 0); break;
        case IDM_SELALL: SendMessage(hEdit, EM_SETSEL, 0, MAKELONG(0, 32767)); break;
        case IDM_TIME: {
            char t[40];
            time_t now = time(NULL);
            struct tm *tm = localtime(&now);
            wsprintf(t, "%02d:%02d %d-%02d-%02d", tm->tm_hour, tm->tm_min,
                     tm->tm_year + 1900, tm->tm_mon + 1, tm->tm_mday);
            SendMessage(hEdit, EM_REPLACESEL, 0, (LPARAM)(LPSTR)t);
            break;
        }
        case IDM_WRAP: {
            /* A new edit control with the other style and the same text. */
            int n = GetWindowTextLength(hEdit);
            HGLOBAL h = GlobalAlloc(GMEM_MOVEABLE, (DWORD)n + 1);
            char FAR *buf = GlobalLock(h);
            BOOL mod = (BOOL)SendMessage(hEdit, EM_GETMODIFY, 0, 0);
            GetWindowText(hEdit, buf, n + 1);
            DestroyWindow(hEdit);
            wrap = !wrap;
            hEdit = make_edit(hwnd);
            SetWindowText(hEdit, buf);
            SendMessage(hEdit, EM_SETMODIFY, mod, 0);
            GlobalUnlock(h);
            GlobalFree(h);
            SetFocus(hEdit);
            break;
        }
        case IDM_ABOUT:
            MessageBox(hwnd, "Note16\n\nA Notepad for rs2's native Win16 layer.", "About Note16", MB_OK);
            break;
        }
        return 0;
    case WM_CLOSE:
        if (may_close(hwnd))
            DestroyWindow(hwnd);
        return 0;
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

    hInst = hInstance;
    while (cmd && (*cmd == ' ' || *cmd == '\t'))
        cmd++;
    if (cmd && *cmd && *cmd != '\r')
        lstrcpy(file, cmd);
    if (!hPrev) {
        wc.style = 0;
        wc.lpfnWndProc = NoteWndProc;
        wc.cbClsExtra = 0;
        wc.cbWndExtra = 0;
        wc.hInstance = hInstance;
        wc.hIcon = LoadIcon(NULL, IDI_APPLICATION);
        wc.hCursor = LoadCursor(NULL, IDC_ARROW);
        wc.hbrBackground = COLOR_WINDOW + 1;
        wc.lpszMenuName = "Note16";
        wc.lpszClassName = "Note16";
        if (!RegisterClass(&wc))
            return 0;
    }
    hwnd = CreateWindow("Note16", "Note16", WS_OVERLAPPEDWINDOW,
                        30, 30, 640, 420, NULL, NULL, hInstance, NULL);
    set_title(hwnd);
    acc = LoadAccelerators(hInstance, "Note16");
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
