/* NOTE16: a small Notepad: a multi-line edit control filling the
 * window, with scroll bars, the clipboard (Edit menu and Ctrl+X/C/V),
 * undo, word wrap, files read and written with _lopen/_lread/_lwrite,
 * a check for unsaved changes, and the common dialogs: Open and Save As
 * (GetOpenFileName, GetSaveFileName), Print and Print Setup (PrintDlg,
 * then StartDoc...EndDoc on the printer DC), Font (ChooseFont) and Find
 * and Replace (FindText, ReplaceText). A test of rs2's native Win16
 * layer (docs/WIN16.md). */

#include <windows.h>
#include <commdlg.h>
#include <string.h>
#include <time.h>
#include "note16.h"

#pragma library("commdlg")

static HINSTANCE hInst;
static HWND hEdit;
static BOOL wrap;
static char file[128] = "C:\\NOTE16.TXT";
static HFONT font;
static LOGFONT logfont;
static COLORREF textcolor, background = RGB(255, 255, 255), custom[16];
static HBRUSH bgbrush;
static UINT findmsg;
static HWND hFind;
static FINDREPLACE fr;
static char findwhat[80], replacewith[80];
static HGLOBAL devmode, devnames;
static BOOL aborted;
static const char filter[] = "Text Files (*.txt)\0*.txt\0Batch Files (*.cmd;*.bat)\0*.cmd;*.bat\0All Files (*.*)\0*.*\0";

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

/* Open (save FALSE) or Save As: the file name, from the common dialog. */
static BOOL ask_name(HWND hwnd, BOOL save)
{
    OPENFILENAME ofn;
    char name[128];
    lstrcpy(name, file);
    memset(&ofn, 0, sizeof ofn);
    ofn.lStructSize = sizeof ofn;
    ofn.hwndOwner = hwnd;
    ofn.lpstrFilter = filter;
    ofn.nFilterIndex = 1;
    ofn.lpstrFile = name;
    ofn.nMaxFile = sizeof name;
    ofn.lpstrDefExt = "txt";
    ofn.Flags = OFN_HIDEREADONLY | OFN_PATHMUSTEXIST | (save ? OFN_OVERWRITEPROMPT : OFN_FILEMUSTEXIST);
    if (!(save ? GetSaveFileName(&ofn) : GetOpenFileName(&ofn)))
        return FALSE;
    lstrcpy(file, name);
    return TRUE;
}

/* The abort procedure: messages go on while printing. */
BOOL FAR PASCAL _export AbortProc(HDC hdc, int code)
{
    MSG msg;
    while (!aborted && PeekMessage(&msg, NULL, 0, 0, PM_REMOVE)) {
        TranslateMessage(&msg);
        DispatchMessage(&msg);
    }
    return !aborted;
}

/* Print the text: PrintDlg for the printer, a page per screenful of
 * lines in a 10 point Courier, a header with the file name. */
static void print(HWND hwnd)
{
    PRINTDLG pd;
    DOCINFO di;
    HDC dc;
    HFONT f, old;
    TEXTMETRIC tm;
    int n, lines, cy, top, y, page = 0, i, len;
    char FAR *buf;
    HGLOBAL h;
    char head[160];
    FARPROC ap;

    memset(&pd, 0, sizeof pd);
    pd.lStructSize = sizeof pd;
    pd.hwndOwner = hwnd;
    pd.hDevMode = devmode;
    pd.hDevNames = devnames;
    pd.Flags = PD_RETURNDC | PD_NOSELECTION | PD_NOPAGENUMS;
    pd.nCopies = 1;
    if (!PrintDlg(&pd))
        return;
    devmode = pd.hDevMode;
    devnames = pd.hDevNames;
    dc = pd.hDC;
    f = CreateFont(-MulDiv(10, GetDeviceCaps(dc, LOGPIXELSY), 72), 0, 0, 0, FW_NORMAL, 0, 0, 0,
                   ANSI_CHARSET, 0, 0, 0, FIXED_PITCH | FF_MODERN, "Courier New");
    old = SelectObject(dc, f);
    GetTextMetrics(dc, &tm);
    cy = tm.tmHeight + tm.tmExternalLeading;
    top = GetDeviceCaps(dc, LOGPIXELSY) / 2;
    lines = (GetDeviceCaps(dc, VERTRES) - 2 * top) / cy - 2;
    n = GetWindowTextLength(hEdit);
    h = GlobalAlloc(GMEM_MOVEABLE, (DWORD)n + 1);
    buf = GlobalLock(h);
    GetWindowText(hEdit, buf, n + 1);
    aborted = FALSE;
    ap = MakeProcInstance((FARPROC)AbortProc, hInst);
    SetAbortProc(dc, (ABORTPROC)ap);
    di.cbSize = sizeof di;
    di.lpszDocName = file;
    di.lpszOutput = NULL;
    if (StartDoc(dc, &di) > 0) {
        i = 0;
        while (i < n || page == 0) {
            StartPage(dc);
            page++;
            wsprintf(head, "%s - page %d", (LPSTR)file, page);
            TextOut(dc, top, top, head, lstrlen(head));
            for (y = 0; y < lines && i < n; y++) {
                /* One line (up to CR LF). */
                len = 0;
                while (i + len < n && buf[i + len] != '\r' && buf[i + len] != '\n')
                    len++;
                TextOut(dc, top, top + (y + 2) * cy, buf + i, len);
                i += len;
                if (i < n && buf[i] == '\r')
                    i++;
                if (i < n && buf[i] == '\n')
                    i++;
            }
            if (EndPage(dc) <= 0)
                break;
        }
        EndDoc(dc);
    }
    FreeProcInstance(ap);
    SelectObject(dc, old);
    DeleteObject(f);
    DeleteDC(dc);
    GlobalUnlock(h);
    GlobalFree(h);
}

static void print_setup(HWND hwnd)
{
    PRINTDLG pd;
    memset(&pd, 0, sizeof pd);
    pd.lStructSize = sizeof pd;
    pd.hwndOwner = hwnd;
    pd.hDevMode = devmode;
    pd.hDevNames = devnames;
    pd.Flags = PD_PRINTSETUP;
    if (PrintDlg(&pd)) {
        devmode = pd.hDevMode;
        devnames = pd.hDevNames;
    }
}

static void choose_font(HWND hwnd)
{
    CHOOSEFONT cf;
    memset(&cf, 0, sizeof cf);
    cf.lStructSize = sizeof cf;
    cf.hwndOwner = hwnd;
    cf.lpLogFont = &logfont;
    cf.Flags = CF_SCREENFONTS | CF_EFFECTS | (font ? CF_INITTOLOGFONTSTRUCT : 0);
    cf.rgbColors = textcolor;
    if (!ChooseFont(&cf))
        return;
    if (font)
        DeleteObject(font);
    font = CreateFontIndirect(&logfont);
    textcolor = cf.rgbColors;
    SendMessage(hEdit, WM_SETFONT, (WPARAM)font, MAKELONG(TRUE, 0));
}

static void choose_color(HWND hwnd)
{
    CHOOSECOLOR cc;
    memset(&cc, 0, sizeof cc);
    cc.lStructSize = sizeof cc;
    cc.hwndOwner = hwnd;
    cc.rgbResult = background;
    cc.lpCustColors = custom;
    cc.Flags = CC_RGBINIT;
    if (!ChooseColor(&cc))
        return;
    background = cc.rgbResult;
    if (bgbrush)
        DeleteObject(bgbrush);
    bgbrush = CreateSolidBrush(background);
    InvalidateRect(hEdit, NULL, TRUE);
}

static void find_dialog(HWND hwnd, BOOL replace)
{
    if (hFind) {
        SetFocus(hFind);
        return;
    }
    memset(&fr, 0, sizeof fr);
    fr.lStructSize = sizeof fr;
    fr.hwndOwner = hwnd;
    fr.Flags = FR_DOWN;
    fr.lpstrFindWhat = findwhat;
    fr.wFindWhatLen = sizeof findwhat;
    fr.lpstrReplaceWith = replacewith;
    fr.wReplaceWithLen = sizeof replacewith;
    hFind = replace ? ReplaceText(&fr) : FindText(&fr);
}

/* Find the text after (or before) the selection and select it. */
static BOOL find_next(HWND hwnd, BOOL down, BOOL matchcase)
{
    int n = GetWindowTextLength(hEdit), k = lstrlen(findwhat), i, j;
    DWORD sel = SendMessage(hEdit, EM_GETSEL, 0, 0);
    HGLOBAL h;
    char FAR *buf;
    BOOL found = FALSE;
    if (k == 0)
        return FALSE;
    h = GlobalAlloc(GMEM_MOVEABLE, (DWORD)n + 1);
    buf = GlobalLock(h);
    GetWindowText(hEdit, buf, n + 1);
    i = down ? HIWORD(sel) : (int)LOWORD(sel) - 1;
    for (; down ? i + k <= n : i >= 0; i += down ? 1 : -1) {
        for (j = 0; j < k; j++) {
            char a = buf[i + j], b = findwhat[j];
            if (!matchcase) {
                a = (char)(DWORD)AnsiUpper((LPSTR)(DWORD)(BYTE)a);
                b = (char)(DWORD)AnsiUpper((LPSTR)(DWORD)(BYTE)b);
            }
            if (a != b)
                break;
        }
        if (j == k) {
            SendMessage(hEdit, EM_SETSEL, 0, MAKELONG(i, i + k));
            found = TRUE;
            break;
        }
    }
    GlobalUnlock(h);
    GlobalFree(h);
    if (!found) {
        char t[120];
        wsprintf(t, "Cannot find \"%s\"", (LPSTR)findwhat);
        MessageBox(hFind ? hFind : hwnd, t, "Note16", MB_OK | MB_ICONINFORMATION);
    }
    return found;
}

/* FINDMSGSTRING from the Find or Replace dialog. */
static void on_find(HWND hwnd, FINDREPLACE FAR *f)
{
    BOOL down = (f->Flags & FR_DOWN) != 0, mc = (f->Flags & FR_MATCHCASE) != 0;
    if (f->Flags & FR_DIALOGTERM) {
        hFind = NULL;
        return;
    }
    if (f->Flags & FR_FINDNEXT) {
        find_next(hwnd, down, mc);
    } else if (f->Flags & FR_REPLACE) {
        DWORD sel = SendMessage(hEdit, EM_GETSEL, 0, 0);
        if (HIWORD(sel) != LOWORD(sel))
            SendMessage(hEdit, EM_REPLACESEL, 0, (LPARAM)(LPSTR)replacewith);
        find_next(hwnd, TRUE, mc);
    } else if (f->Flags & FR_REPLACEALL) {
        int count = 0;
        char t[60];
        SendMessage(hEdit, EM_SETSEL, 0, MAKELONG(0, 0));
        lstrcpy(t, findwhat);
        while (count < 1000) {
            int n = GetWindowTextLength(hEdit), k = lstrlen(findwhat), i, j;
            DWORD sel = SendMessage(hEdit, EM_GETSEL, 0, 0);
            HGLOBAL h = GlobalAlloc(GMEM_MOVEABLE, (DWORD)n + 1);
            char FAR *buf = GlobalLock(h);
            BOOL found = FALSE;
            GetWindowText(hEdit, buf, n + 1);
            for (i = HIWORD(sel); k && i + k <= n && !found; i++) {
                for (j = 0; j < k && buf[i + j] == findwhat[j]; j++)
                    ;
                if (j == k) {
                    SendMessage(hEdit, EM_SETSEL, 0, MAKELONG(i, i + k));
                    found = TRUE;
                }
            }
            GlobalUnlock(h);
            GlobalFree(h);
            if (!found)
                break;
            SendMessage(hEdit, EM_REPLACESEL, 0, (LPARAM)(LPSTR)replacewith);
            count++;
        }
        wsprintf(t, "%d replaced.", count);
        MessageBox(hFind, t, "Note16", MB_OK | MB_ICONINFORMATION);
    }
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
    if (msg == findmsg && findmsg) {
        on_find(hwnd, (FINDREPLACE FAR *)lp);
        return 0;
    }
    switch (msg) {
    case WM_CTLCOLOR:
        if ((font || bgbrush) && (HWND)LOWORD(lp) == hEdit) {
            SetTextColor((HDC)wp, textcolor);
            SetBkColor((HDC)wp, background);
            return (LONG)(WORD)(bgbrush ? bgbrush : GetStockObject(WHITE_BRUSH));
        }
        break;
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
            if (may_close(hwnd) && ask_name(hwnd, FALSE))
                load(hwnd);
            break;
        case IDM_SAVE: save(hwnd); break;
        case IDM_SAVEAS: if (ask_name(hwnd, TRUE)) save(hwnd); break;
        case IDM_PRINT: print(hwnd); break;
        case IDM_PRINTSETUP: print_setup(hwnd); break;
        case IDM_FONT: choose_font(hwnd); break;
        case IDM_COLOR: choose_color(hwnd); break;
        case IDM_FIND: find_dialog(hwnd, FALSE); break;
        case IDM_REPLACE: find_dialog(hwnd, TRUE); break;
        case IDM_FINDNEXT:
            if (findwhat[0])
                find_next(hwnd, TRUE, FALSE);
            else
                find_dialog(hwnd, FALSE);
            break;
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
            if (font)
                SendMessage(hEdit, WM_SETFONT, (WPARAM)font, 0);
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
    findmsg = RegisterWindowMessage("commdlg_FindReplace");
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
        if (hFind && IsDialogMessage(hFind, &msg))
            continue;
        if (!TranslateAccelerator(hwnd, acc, &msg)) {
            TranslateMessage(&msg);
            DispatchMessage(&msg);
        }
    }
    return msg.wParam;
}
