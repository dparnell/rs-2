/* W16TEST: checks of rs2's native Win16 layer (docs/WIN16.md). A
 * Windows 3.x program that calls KERNEL, USER and GDI and the DOS API
 * (stage 3: COMMDLG, printing, atoms, the clipboard's formats, DDEML)
 * and writes PASS/FAIL lines to standard output (INT 21h), ending with
 * "w16test done: N failure(s)". Run it with
 * cargo xtask run --release --headless --timeout 150 --init "RS2\W16TEST.EXE"
 */

#define OEMRESOURCE
#include <windows.h>
#include <commdlg.h>
#include <cderr.h>
#include <drivinit.h>
/* Watcom's ddeml.h declares DdeAccessData near (Windows': BYTE FAR *). */
#define DdeAccessData DdeAccessData_near
#include <ddeml.h>
#undef DdeAccessData
BYTE FAR * WINAPI DdeAccessData(HDDEDATA, DWORD FAR *);
#include <dde.h>
#include <string.h>
#include <stdio.h>
#include <io.h>
#include <fcntl.h>
#include <lzexpand.h>
#include <ver.h>
#include <toolhelp.h>
#include <shellapi.h>
#include "w16test.h"

#pragma library("commdlg")
#pragma library("ddeml")
#pragma library("toolhelp")
#pragma library("shell")

static int failures;
static char line[160];

static void out(const char *s)
{
    write(1, s, strlen(s));
}

static void check(int ok, const char *what)
{
    out(ok ? "PASS " : "FAIL ");
    out(what);
    out("\r\n");
    if (!ok)
        failures++;
}

static int created, destroyed, user_msgs, posted_seen, timer_seen, painted;
static LONG create_param;

LONG FAR PASCAL _export TestWndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_CREATE:
        created++;
        create_param = (LONG)((CREATESTRUCT FAR *)lp)->lpCreateParams;
        return 0;
    case WM_USER:
        user_msgs++;
        return wp + 1;
    case WM_USER + 1:
        posted_seen++;
        return 0;
    case WM_TIMER:
        timer_seen++;
        return 0;
    case WM_PAINT: {
        PAINTSTRUCT ps;
        BeginPaint(hwnd, &ps);
        painted++;
        EndPaint(hwnd, &ps);
        return 0;
    }
    case WM_DESTROY:
        destroyed++;
        return 0;
    }
    return DefWindowProc(hwnd, msg, wp, lp);
}

static void kernel_tests(void)
{
    DWORD v = GetVersion();
    HLOCAL hl;
    HGLOBAL hg;
    char NEAR *p;
    char FAR *g;
    char huge *h;
    char buf[32];

    check(LOBYTE(LOWORD(v)) == 3 && HIBYTE(LOWORD(v)) == 10, "GetVersion is Windows 3.10");
    check((GetWinFlags() & WF_PMODE) != 0, "GetWinFlags: protected mode");

    hl = LocalAlloc(LMEM_MOVEABLE | LMEM_ZEROINIT, 100);
    p = LocalLock(hl);
    check(hl != 0 && p != NULL && p[0] == 0 && p[99] == 0, "LocalAlloc/LocalLock (zeroed)");
    check(LocalSize(hl) >= 100, "LocalSize");
    strcpy(p, "local");
    LocalUnlock(hl);
    hl = LocalReAlloc(hl, 2000, LMEM_MOVEABLE);
    p = LocalLock(hl);
    check(hl != 0 && strcmp(p, "local") == 0 && LocalSize(hl) >= 2000, "LocalReAlloc keeps the data");
    LocalUnlock(hl);
    check(LocalFree(hl) == 0, "LocalFree");

    hg = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, 100000L);
    g = GlobalLock(hg);
    check(hg != 0 && g != NULL, "GlobalAlloc 100000 bytes");
    check(GlobalSize(hg) >= 100000L, "GlobalSize");
    h = (char huge *)g;
    h[70000L] = 'x';
    check(h[70000L] == 'x' && h[69999L] == 0, "a huge pointer across 64 KB");
    GlobalUnlock(hg);
    hg = GlobalReAlloc(hg, 120000L, GMEM_MOVEABLE);
    h = (char huge *)GlobalLock(hg);
    check(hg != 0 && h[70000L] == 'x', "GlobalReAlloc keeps the data");
    GlobalUnlock(hg);
    check(GlobalFree(hg) == 0, "GlobalFree");

    lstrcpy(buf, "Win");
    lstrcat(buf, "16");
    check(lstrlen(buf) == 5 && strcmp(buf, "Win16") == 0, "lstrcpy, lstrcat, lstrlen");

    check(GetModuleHandle("USER") != 0 && GetProcAddress(GetModuleHandle("USER"), "MESSAGEBOX") != NULL,
          "GetModuleHandle(USER), GetProcAddress");
    check(GetModuleFileName(GetModuleHandle(NULL), buf, sizeof buf) > 0, "GetModuleFileName");
}

static void dos_tests(void)
{
    int fd;
    char buf[16];
    int n;

    fd = open("C:\\W16TEST.TMP", O_CREAT | O_TRUNC | O_RDWR | O_BINARY, 0666);
    check(fd >= 0, "INT 21h: create a file");
    if (fd < 0)
        return;
    check(write(fd, "hello, file", 11) == 11, "INT 21h: write");
    check(lseek(fd, 7L, SEEK_SET) == 7L, "INT 21h: seek");
    n = read(fd, buf, sizeof buf);
    check(n == 4 && memcmp(buf, "file", 4) == 0, "INT 21h: read");
    close(fd);
    check(unlink("C:\\W16TEST.TMP") == 0, "INT 21h: delete");
}

static void user_tests(HINSTANCE hInst)
{
    WNDCLASS wc;
    HWND hwnd;
    RECT rc;
    POINT pt;
    MSG msg;
    HDC hdc;
    TEXTMETRIC tm;
    DWORD ext;

    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = TestWndProc;
    wc.hInstance = hInst;
    wc.hCursor = LoadCursor(NULL, IDC_ARROW);
    wc.hbrBackground = GetStockObject(WHITE_BRUSH);
    wc.lpszClassName = "W16Test";
    wc.cbWndExtra = 4;
    check(RegisterClass(&wc) != 0, "RegisterClass");
    check(RegisterClass(&wc) == 0, "RegisterClass twice fails");

    hwnd = CreateWindow("W16Test", "W16TEST", WS_OVERLAPPEDWINDOW, 100, 80, 300, 200,
                        NULL, NULL, hInst, (LPVOID)0x12345678L);
    check(hwnd != 0, "CreateWindow");
    check(created == 1 && create_param == 0x12345678L, "WM_CREATE with the CREATESTRUCT");
    GetClientRect(hwnd, &rc);
    check(rc.left == 0 && rc.top == 0 && rc.right > 250 && rc.right < 300 && rc.bottom > 120 && rc.bottom < 200,
          "GetClientRect: the client area of a 300x200 window");
    GetWindowRect(hwnd, &rc);
    sprintf(line, "GetWindowRect: (100,80)-(400,280), is (%d,%d)-(%d,%d)", rc.left, rc.top, rc.right, rc.bottom);
    check(rc.left == 100 && rc.top == 80 && rc.right == 400 && rc.bottom == 280, line);

    pt.x = 10;
    pt.y = 20;
    ClientToScreen(hwnd, &pt);
    check(pt.x > 100 && pt.y > 80 + 20, "ClientToScreen: y grows down");
    ScreenToClient(hwnd, &pt);
    check(pt.x == 10 && pt.y == 20, "ScreenToClient undoes it");

    check(SetWindowLong(hwnd, 0, 0xCAFEL) == 0 && GetWindowLong(hwnd, 0) == 0xCAFEL, "window extra bytes");
    check(GetWindowLong(hwnd, GWL_WNDPROC) == (LONG)(FARPROC)TestWndProc, "GWL_WNDPROC");

    check(SendMessage(hwnd, WM_USER, 41, 0) == 42 && user_msgs == 1, "SendMessage");
    check(PostMessage(hwnd, WM_USER + 1, 0, 0), "PostMessage");
    while (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE))
        DispatchMessage(&msg);
    check(posted_seen == 1, "PeekMessage and DispatchMessage of a posted message");

    ShowWindow(hwnd, SW_SHOWNORMAL);
    UpdateWindow(hwnd);
    check(painted > 0, "UpdateWindow sends WM_PAINT");
    check(IsWindowVisible(hwnd), "ShowWindow");

    hdc = GetDC(hwnd);
    check(hdc != 0, "GetDC");
    check(GetTextMetrics(hdc, &tm) && tm.tmHeight > 8, "GetTextMetrics");
    ext = GetTextExtent(hdc, "Hello", 5);
    check(LOWORD(ext) > 10 && HIWORD(ext) == (WORD)tm.tmHeight, "GetTextExtent");
    check(TextOut(hdc, 5, 5, "W16TEST", 7), "TextOut");
    SetRect(&rc, 0, 0, 100, 0);
    DrawText(hdc, "two\nlines", -1, &rc, DT_CALCRECT);
    check(rc.bottom == 2 * tm.tmHeight, "DrawText DT_CALCRECT: two lines");
    ReleaseDC(hwnd, hdc);

    check(SetTimer(hwnd, 7, 100, NULL) != 0, "SetTimer");
    while (!timer_seen && GetMessage(&msg, NULL, 0, 0))
        DispatchMessage(&msg);
    KillTimer(hwnd, 7);
    check(timer_seen > 0, "WM_TIMER through GetMessage");

    check(DestroyWindow(hwnd) && destroyed == 1, "DestroyWindow sends WM_DESTROY");
    check(!IsWindow(hwnd), "IsWindow after DestroyWindow");

    PostQuitMessage(3);
    check(!GetMessage(&msg, NULL, 0, 0) && msg.message == WM_QUIT && msg.wParam == 3, "PostQuitMessage: GetMessage returns 0");
}

/* ---- Stage 2: resources, menus, accelerators, dialogs, controls ---- */

static HINSTANCE hInstance;
static int commands[1000];
static WORD last_notify, last_notify_id;
static int init_dialogs, ctlcolors, sub_msgs;
static LPARAM init_param;
static FARPROC old_edit;

LONG FAR PASCAL _export Stage2WndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_COMMAND:
        if (wp < 1000)
            commands[wp] = HIWORD(lp) == 1 ? 2 : 1;
        return 0;
    case WM_PAINT: {
        PAINTSTRUCT ps;
        BeginPaint(hwnd, &ps);
        EndPaint(hwnd, &ps);
        return 0;
    }
    }
    return DefWindowProc(hwnd, msg, wp, lp);
}

BOOL FAR PASCAL _export TestDlgProc(HWND dlg, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_INITDIALOG:
        init_dialogs++;
        init_param = lp;
        return TRUE;
    case WM_CTLCOLOR:
        ctlcolors++;
        return FALSE;
    case WM_COMMAND:
        if (wp < 1000)
            commands[wp]++;
        last_notify = HIWORD(lp);
        last_notify_id = wp;
        return TRUE;
    }
    return FALSE;
}

/* The modal dialog: ends itself with the parameter it was given. */
BOOL FAR PASCAL _export ModalDlgProc(HWND dlg, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_INITDIALOG:
        init_param = lp;
        PostMessage(dlg, WM_COMMAND, IDOK, 0);
        return TRUE;
    case WM_COMMAND:
        if (wp == IDOK) {
            EndDialog(dlg, (int)init_param);
            return TRUE;
        }
        break;
    }
    return FALSE;
}

LONG FAR PASCAL _export SubEditProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    if (msg == WM_CHAR)
        sub_msgs++;
    return CallWindowProc(old_edit, hwnd, msg, wp, lp);
}

/* IsDialogMessage on a made-up key message. */
static BOOL dlg_key(HWND dlg, HWND to, UINT msg, WPARAM key)
{
    MSG m;
    memset(&m, 0, sizeof m);
    m.hwnd = to;
    m.message = msg;
    m.wParam = key;
    m.lParam = 1;
    return IsDialogMessage(dlg, &m);
}

static void click(HWND h)
{
    SendMessage(h, WM_LBUTTONDOWN, MK_LBUTTON, MAKELONG(3, 3));
    SendMessage(h, WM_LBUTTONUP, 0, MAKELONG(3, 3));
}

static void resource_tests(HWND hwnd)
{
    char buf[64];
    HMENU m, sub, pop;
    HACCEL acc;
    MSG msg;

    check(LoadString(hInstance, IDS_HELLO, buf, sizeof buf) == 13 && strcmp(buf, "Hello strings") == 0, "LoadString");
    check(LoadString(hInstance, IDS_SEVENTEEN, buf, sizeof buf) == 9 && strcmp(buf, "seventeen") == 0, "LoadString in a second block");
    check(LoadString(hInstance, 99, buf, sizeof buf) == 0, "LoadString of a missing string");

    m = LoadMenu(hInstance, "TestMenu");
    check(m != 0 && GetMenuItemCount(m) == 2, "LoadMenu: two pop-ups");
    sub = GetSubMenu(m, 0);
    check(sub != 0 && GetMenuItemCount(sub) == 3 && GetMenuItemID(sub, 1) == IDM_SAVE, "GetSubMenu, GetMenuItemID");
    check(GetMenuItemID(m, 0) == (UINT)-1, "GetMenuItemID of a pop-up is -1");
    check(GetMenuString(sub, IDM_OPEN, buf, sizeof buf, MF_BYCOMMAND) == 5 && strcmp(buf, "&Open") == 0, "GetMenuString");
    check(CheckMenuItem(sub, IDM_OPEN, MF_CHECKED) == MF_UNCHECKED && (GetMenuState(sub, IDM_OPEN, MF_BYCOMMAND) & MF_CHECKED),
          "CheckMenuItem, GetMenuState");
    check(EnableMenuItem(m, IDM_SAVE, MF_GRAYED) == MF_ENABLED && (GetMenuState(sub, 1, MF_BYPOSITION) & MF_GRAYED),
          "EnableMenuItem (in a submenu, by command)");
    check(AppendMenu(sub, MF_STRING, 299, "&Added") && GetMenuItemCount(sub) == 4, "AppendMenu");
    check(DeleteMenu(sub, 299, MF_BYCOMMAND) && GetMenuItemCount(sub) == 3, "DeleteMenu");
    pop = CreatePopupMenu();
    AppendMenu(pop, MF_STRING, 401, "Four");
    check(AppendMenu(m, MF_POPUP, (UINT)pop, "&More") && GetSubMenu(m, 2) == pop && GetMenuItemCount(m) == 3,
          "CreatePopupMenu, AppendMenu MF_POPUP");
    check(SetMenu(hwnd, m) && GetMenu(hwnd) == m, "SetMenu, GetMenu");

    acc = LoadAccelerators(hInstance, "TestAcc");
    check(acc != 0, "LoadAccelerators");
    memset(&msg, 0, sizeof msg);
    msg.hwnd = hwnd;
    msg.message = WM_KEYDOWN;
    msg.wParam = VK_F5;
    check(TranslateAccelerator(hwnd, acc, &msg) && commands[IDA_F5] == 2, "TranslateAccelerator: VK_F5 -> WM_COMMAND from an accelerator");
    msg.message = WM_CHAR;
    msg.wParam = 0x11;
    check(TranslateAccelerator(hwnd, acc, &msg) && commands[IDA_CTRLQ] == 2, "TranslateAccelerator: ^Q");
    msg.message = WM_KEYDOWN;
    msg.wParam = 'A';
    check(!TranslateAccelerator(hwnd, acc, &msg), "TranslateAccelerator: other keys pass");
}

static void dialog_tests(HWND hwnd)
{
    FARPROC proc = MakeProcInstance((FARPROC)TestDlgProc, hInstance);
    HWND dlg, edit, multi, keep, list, combo, drop, sb, f;
    char buf[64];
    BOOL ok;
    int lo, hi;
    DWORD sel;

    dlg = CreateDialogParam(hInstance, "TestDlg", hwnd, (DLGPROC)proc, 0x5678L);
    check(dlg != 0 && init_dialogs == 1 && init_param == 0x5678L, "CreateDialogParam: WM_INITDIALOG with the parameter");
    if (!dlg)
        return;
    ShowWindow(dlg, SW_SHOW);
    UpdateWindow(dlg);
    check(ctlcolors > 0, "the controls ask for WM_CTLCOLOR");
    edit = GetDlgItem(dlg, IDC_EDIT);
    multi = GetDlgItem(dlg, IDC_MULTI);
    keep = GetDlgItem(dlg, IDC_KEEP);
    list = GetDlgItem(dlg, IDC_LIST);
    combo = GetDlgItem(dlg, IDC_COMBO);
    drop = GetDlgItem(dlg, IDC_DROP);
    sb = GetDlgItem(dlg, IDC_SCROLL);
    check(edit && multi && keep && list && combo && drop && sb, "GetDlgItem: every control");
    check(GetDlgCtrlID(list) == IDC_LIST && GetParent(list) == dlg && IsChild(dlg, list), "GetDlgCtrlID, GetParent, IsChild");
    GetClassName(list, buf, sizeof buf);
    check(lstrcmpi(buf, "listbox") == 0, "GetClassName of a built-in control");
    check(SendMessage(dlg, DM_GETDEFID, 0, 0) == MAKELONG(IDOK, DC_HASDEFID), "DM_GETDEFID: the default push button");

    SetDlgItemText(dlg, IDC_EDIT, "hello");
    check(GetDlgItemText(dlg, IDC_EDIT, buf, sizeof buf) == 5 && strcmp(buf, "hello") == 0, "SetDlgItemText, GetDlgItemText");
    check(GetWindowTextLength(edit) == 5, "GetWindowTextLength of an edit control");
    SetDlgItemInt(dlg, IDC_EDIT, (UINT)-42, TRUE);
    check(GetDlgItemInt(dlg, IDC_EDIT, &ok, TRUE) == (UINT)-42 && ok, "SetDlgItemInt, GetDlgItemInt (signed)");
    SetDlgItemText(dlg, IDC_EDIT, "12x");
    GetDlgItemInt(dlg, IDC_EDIT, &ok, FALSE);
    check(!ok, "GetDlgItemInt of a non-number");

    /* EDIT */
    SetWindowText(edit, "abcdef");
    SendMessage(edit, EM_SETSEL, 0, MAKELONG(1, 3));
    sel = SendMessage(edit, EM_GETSEL, 0, 0);
    check(LOWORD(sel) == 1 && HIWORD(sel) == 3, "EM_SETSEL, EM_GETSEL");
    SendMessage(edit, EM_REPLACESEL, 0, (LPARAM)(LPSTR)"XY");
    GetWindowText(edit, buf, sizeof buf);
    check(strcmp(buf, "aXYdef") == 0, "EM_REPLACESEL");
    last_notify = 0;
    SendMessage(edit, WM_CHAR, 'z', 1);
    GetWindowText(edit, buf, sizeof buf);
    check(strcmp(buf, "aXYzdef") == 0 && last_notify == EN_CHANGE && last_notify_id == IDC_EDIT, "WM_CHAR types; EN_CHANGE");
    check(SendMessage(edit, EM_GETMODIFY, 0, 0), "EM_GETMODIFY after typing");
    SendMessage(edit, WM_KEYDOWN, VK_HOME, 1);
    SendMessage(edit, WM_KEYDOWN, VK_DELETE, 1);
    GetWindowText(edit, buf, sizeof buf);
    check(strcmp(buf, "XYzdef") == 0, "VK_HOME, VK_DELETE");
    check(SendMessage(edit, EM_CANUNDO, 0, 0) && SendMessage(edit, EM_UNDO, 0, 0), "EM_UNDO");
    GetWindowText(edit, buf, sizeof buf);
    check(strcmp(buf, "aXYzdef") == 0, "EM_UNDO restores the text");
    SendMessage(edit, EM_LIMITTEXT, 8, 0);
    SendMessage(edit, WM_KEYDOWN, VK_END, 1);
    SendMessage(edit, WM_CHAR, '1', 1);
    SendMessage(edit, WM_CHAR, '2', 1);
    check(GetWindowTextLength(edit) == 8, "EM_LIMITTEXT stops typing");

    SetWindowText(multi, "one\r\ntwo\r\nthree");
    check(SendMessage(multi, EM_GETLINECOUNT, 0, 0) == 3, "multi-line EM_GETLINECOUNT");
    check(SendMessage(multi, EM_LINEINDEX, 1, 0) == 5 && SendMessage(multi, EM_LINELENGTH, 5, 0) == 3, "EM_LINEINDEX, EM_LINELENGTH");
    check(SendMessage(multi, EM_LINEFROMCHAR, 10, 0) == 2 && SendMessage(multi, EM_LINEFROMCHAR, 9, 0) == 1, "EM_LINEFROMCHAR");
    *(WORD *)buf = sizeof buf;
    lo = (int)SendMessage(multi, EM_GETLINE, 2, (LPARAM)(LPSTR)buf);
    buf[lo] = 0;
    check(lo == 5 && strcmp(buf, "three") == 0, "EM_GETLINE");

    /* The clipboard, through the edit controls and directly. */
    SendMessage(multi, EM_SETSEL, 0, MAKELONG(5, 8));
    SendMessage(multi, WM_COPY, 0, 0);
    SetWindowText(edit, "");
    SendMessage(edit, WM_PASTE, 0, 0);
    GetWindowText(edit, buf, sizeof buf);
    check(strcmp(buf, "two") == 0 && IsClipboardFormatAvailable(CF_TEXT), "WM_COPY, WM_PASTE (PM's clipboard)");
    if (OpenClipboard(hwnd)) {
        HGLOBAL h = GlobalAlloc(GMEM_MOVEABLE | GMEM_DDESHARE, 16);
        lstrcpy(GlobalLock(h), "clip16");
        GlobalUnlock(h);
        EmptyClipboard();
        SetClipboardData(CF_TEXT, h);
        CloseClipboard();
    }
    buf[0] = 0;
    if (OpenClipboard(hwnd)) {
        HGLOBAL h = GetClipboardData(CF_TEXT);
        if (h) {
            lstrcpy(buf, GlobalLock(h));
            GlobalUnlock(h);
        }
        CloseClipboard();
    }
    check(strcmp(buf, "clip16") == 0, "SetClipboardData, GetClipboardData (CF_TEXT)");

    /* BUTTON */
    CheckDlgButton(dlg, IDC_KEEP, 1);
    check(IsDlgButtonChecked(dlg, IDC_KEEP) == 1, "CheckDlgButton, IsDlgButtonChecked");
    last_notify = 99;
    click(keep);
    check(IsDlgButtonChecked(dlg, IDC_KEEP) == 0 && last_notify == BN_CLICKED && last_notify_id == IDC_KEEP,
          "a click toggles an auto check box; BN_CLICKED");
    CheckRadioButton(dlg, IDC_R1, IDC_R3, IDC_R2);
    check(!IsDlgButtonChecked(dlg, IDC_R1) && IsDlgButtonChecked(dlg, IDC_R2) && !IsDlgButtonChecked(dlg, IDC_R3), "CheckRadioButton");
    click(GetDlgItem(dlg, IDC_R3));
    check(!IsDlgButtonChecked(dlg, IDC_R2) && IsDlgButtonChecked(dlg, IDC_R3), "an auto radio button unchecks its group");

    /* LISTBOX */
    SendMessage(list, LB_ADDSTRING, 0, (LPARAM)(LPSTR)"pear");
    SendMessage(list, LB_ADDSTRING, 0, (LPARAM)(LPSTR)"apple");
    SendMessage(list, LB_ADDSTRING, 0, (LPARAM)(LPSTR)"fig");
    SendMessage(list, LB_GETTEXT, 0, (LPARAM)(LPSTR)buf);
    check(SendMessage(list, LB_GETCOUNT, 0, 0) == 3 && strcmp(buf, "apple") == 0, "LB_ADDSTRING sorts (LBS_SORT)");
    check(SendMessage(list, LB_FINDSTRING, (WPARAM)-1, (LPARAM)(LPSTR)"fi") == 1, "LB_FINDSTRING");
    check(SendMessage(list, LB_SETCURSEL, 2, 0) == 2 && SendMessage(list, LB_GETCURSEL, 0, 0) == 2, "LB_SETCURSEL, LB_GETCURSEL");
    SendMessage(list, LB_SETITEMDATA, 1, 0x1234L);
    check(SendMessage(list, LB_GETITEMDATA, 1, 0) == 0x1234L, "LB_SETITEMDATA, LB_GETITEMDATA");
    check(SendMessage(list, LB_DELETESTRING, 0, 0) == 2 && SendMessage(list, LB_GETTEXTLEN, 1, 0) == 4, "LB_DELETESTRING, LB_GETTEXTLEN");
    last_notify = 0;
    SendMessage(list, WM_KEYDOWN, VK_HOME, 1);
    check(SendMessage(list, LB_GETCURSEL, 0, 0) == 0 && last_notify == LBN_SELCHANGE, "a key selects; LBN_SELCHANGE");

    /* COMBOBOX */
    SendMessage(combo, CB_ADDSTRING, 0, (LPARAM)(LPSTR)"red");
    SendMessage(combo, CB_ADDSTRING, 0, (LPARAM)(LPSTR)"green");
    SendMessage(combo, CB_ADDSTRING, 0, (LPARAM)(LPSTR)"blue");
    SendMessage(combo, CB_SETCURSEL, 1, 0);
    GetWindowText(combo, buf, sizeof buf);
    check(SendMessage(combo, CB_GETCOUNT, 0, 0) == 3 && strcmp(buf, "green") == 0, "CB_SETCURSEL sets the edit control's text");
    check(SendMessage(combo, CB_FINDSTRINGEXACT, (WPARAM)-1, (LPARAM)(LPSTR)"blue") == 2, "CB_FINDSTRINGEXACT");
    SendMessage(combo, CB_SHOWDROPDOWN, TRUE, 0);
    check(SendMessage(combo, CB_GETDROPPEDSTATE, 0, 0), "CB_SHOWDROPDOWN drops the list");
    SendMessage(combo, CB_SHOWDROPDOWN, FALSE, 0);
    check(!SendMessage(combo, CB_GETDROPPEDSTATE, 0, 0), "CB_SHOWDROPDOWN closes it");
    SendMessage(drop, CB_ADDSTRING, 0, (LPARAM)(LPSTR)"small");
    SendMessage(drop, CB_ADDSTRING, 0, (LPARAM)(LPSTR)"large");
    SendMessage(drop, CB_SETCURSEL, 1, 0);
    GetWindowText(drop, buf, sizeof buf);
    SendMessage(drop, CB_GETLBTEXT, 0, (LPARAM)(LPSTR)(buf + 20));
    check(strcmp(buf, "large") == 0 && strcmp(buf + 20, "small") == 0, "drop-down list: CB_GETLBTEXT, the window text");

    /* SCROLLBAR */
    SetScrollRange(sb, SB_CTL, 0, 50, FALSE);
    check(SetScrollPos(sb, SB_CTL, 20, TRUE) == 0 && GetScrollPos(sb, SB_CTL) == 20, "SetScrollPos, GetScrollPos (SB_CTL)");
    GetScrollRange(sb, SB_CTL, &lo, &hi);
    check(lo == 0 && hi == 50, "GetScrollRange (SB_CTL)");
    SetScrollPos(sb, SB_CTL, 80, TRUE);
    check(GetScrollPos(sb, SB_CTL) == 50, "SetScrollPos clamps to the range");

    /* The dialog manager's keyboard interface. */
    SetFocus(edit);
    check(GetFocus() == edit, "SetFocus, GetFocus");
    check(dlg_key(dlg, edit, WM_KEYDOWN, VK_TAB) && GetFocus() == multi, "IsDialogMessage: Tab to the next control");
    check(GetNextDlgTabItem(dlg, multi, FALSE) == keep && GetNextDlgTabItem(dlg, keep, TRUE) == multi, "GetNextDlgTabItem");
    f = GetDlgItem(dlg, IDC_R3);
    SetFocus(f);
    check(dlg_key(dlg, f, WM_KEYDOWN, VK_UP) && GetFocus() == GetDlgItem(dlg, IDC_R2) && IsDlgButtonChecked(dlg, IDC_R2),
          "IsDialogMessage: an arrow moves within the group (and checks the radio button)");
    check(GetNextDlgGroupItem(dlg, GetDlgItem(dlg, IDC_R3), FALSE) == GetDlgItem(dlg, IDC_R1), "GetNextDlgGroupItem wraps in the group");
    commands[IDOK] = 0;
    check(dlg_key(dlg, edit, WM_KEYDOWN, VK_RETURN) && commands[IDOK] == 1, "IsDialogMessage: Enter is the default button");
    commands[IDCANCEL] = 0;
    check(dlg_key(dlg, edit, WM_KEYDOWN, VK_ESCAPE) && commands[IDCANCEL] == 1, "IsDialogMessage: Esc is IDCANCEL");
    last_notify_id = 0;
    check(dlg_key(dlg, edit, WM_SYSCHAR, 'k') && last_notify_id == IDC_KEEP && IsDlgButtonChecked(dlg, IDC_KEEP),
          "IsDialogMessage: Alt+K clicks &Keep");
    check(dlg_key(dlg, keep, WM_SYSCHAR, 'l') && GetFocus() == edit, "IsDialogMessage: a static's mnemonic focuses the next control");

    /* Subclassing a built-in control. */
    old_edit = (FARPROC)SetWindowLong(edit, GWL_WNDPROC, (LONG)(FARPROC)SubEditProc);
    check(old_edit != NULL && GetWindowLong(edit, GWL_WNDPROC) == (LONG)(FARPROC)SubEditProc, "SetWindowLong(GWL_WNDPROC) subclasses");
    SetWindowText(edit, "");
    SendMessage(edit, WM_CHAR, 'q', 1);
    GetWindowText(edit, buf, sizeof buf);
    check(sub_msgs == 1 && strcmp(buf, "q") == 0, "CallWindowProc to the built-in procedure");
    SetWindowLong(edit, GWL_WNDPROC, (LONG)old_edit);

    DestroyWindow(dlg);
    check(!IsWindow(dlg) && !IsWindow(edit), "DestroyWindow of a dialog and its controls");
    FreeProcInstance(proc);

    proc = MakeProcInstance((FARPROC)ModalDlgProc, hInstance);
    check(DialogBoxParam(hInstance, "TestDlg", hwnd, (DLGPROC)proc, 42L) == 42, "DialogBoxParam: EndDialog's result");
    FreeProcInstance(proc);
}

static void window_tests(HWND hwnd)
{
    HWND w;
    RECT rc;
    int lo, hi;
    HDC hdc;
    COLORREF c;
    POINT pt;
    char buf[64];

    w = CreateWindow("Stage2", "scroll", WS_OVERLAPPEDWINDOW | WS_VSCROLL | WS_HSCROLL | WS_VISIBLE,
                     50, 50, 300, 200, NULL, NULL, hInstance, NULL);
    check(w != 0, "a window with scroll bars");
    GetClientRect(w, &rc);
    check(rc.right < 290 && rc.bottom < 170, "the scroll bars take room from the client area");
    SetScrollRange(w, SB_VERT, 0, 50, FALSE);
    check(SetScrollPos(w, SB_VERT, 20, TRUE) == 0 && GetScrollPos(w, SB_VERT) == 20, "SetScrollPos, GetScrollPos (SB_VERT)");
    GetScrollRange(w, SB_VERT, &lo, &hi);
    check(lo == 0 && hi == 50, "GetScrollRange (SB_VERT)");
    SetScrollRange(w, SB_HORZ, 0, 10, TRUE);
    SetScrollPos(w, SB_HORZ, 3, TRUE);
    check(GetScrollPos(w, SB_HORZ) == 3, "SB_HORZ");

    /* The background: the class brush (white), not black. */
    UpdateWindow(w);
    hdc = GetDC(w);
    c = GetPixel(hdc, 20, 20);
    ReleaseDC(w, hdc);
    wsprintf(buf, "the background is the class brush (white), is %06lX", c);
    check(c == RGB(255, 255, 255), buf);

    CreateCaret(w, NULL, 2, 14);
    SetCaretPos(7, 9);
    GetCaretPos(&pt);
    check(pt.x == 7 && pt.y == 9, "CreateCaret, SetCaretPos, GetCaretPos");
    DestroyCaret();

    check(GetWindow(hwnd, GW_HWNDNEXT) != 0 || GetWindow(hwnd, GW_HWNDPREV) != 0, "GetWindow over the top-level windows");
    wsprintf(buf, "%d %s %04x %ld %c", -5, (LPSTR)"str", 0xbeef, 70000L, 'x');
    check(strcmp(buf, "-5 str beef 70000 x") == 0, "wsprintf");
    check(lstrcmpi("Hello", "hELLO") == 0 && lstrcmp("a", "b") < 0, "lstrcmp, lstrcmpi");
    DestroyWindow(w);
}

static void stage2_tests(HINSTANCE hInst)
{
    WNDCLASS wc;
    HWND hwnd;

    hInstance = hInst;
    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = Stage2WndProc;
    wc.hInstance = hInst;
    wc.hCursor = LoadCursor(NULL, IDC_ARROW);
    wc.hbrBackground = GetStockObject(WHITE_BRUSH);
    wc.lpszClassName = "Stage2";
    RegisterClass(&wc);
    hwnd = CreateWindow("Stage2", "W16TEST stage 2", WS_OVERLAPPEDWINDOW | WS_VISIBLE, 20, 20, 400, 300,
                        NULL, NULL, hInst, NULL);
    resource_tests(hwnd);
    dialog_tests(hwnd);
    window_tests(hwnd);
    DestroyWindow(hwnd);
}


/* ---- Stage 3 ---- */

static HWND s3wnd;
static int hook_inits, hook_ok, findmsgs, renders, destroys;
static DWORD find_flags;
static char find_seen[40];
static UINT findmsg;
static UINT reg_fmt;

/* Hooks that check the dialog's controls and press OK (or Cancel). */
UINT CALLBACK _export FileHook(HWND dlg, UINT msg, WPARAM wp, LPARAM lp)
{
    if (msg == WM_INITDIALOG) {
        OPENFILENAME FAR *o = (OPENFILENAME FAR *)lp;
        hook_inits++;
        hook_ok = GetDlgItem(dlg, 0x480) && GetDlgItem(dlg, 0x460) && GetDlgItem(dlg, 0x461) &&
                  GetDlgItem(dlg, 0x470) && GetDlgItem(dlg, 0x471) &&
                  SendDlgItemMessage(dlg, 0x460, LB_GETCOUNT, 0, 0) > 0 && o->lCustData == 1234;
        SetDlgItemText(dlg, 0x480, (LPSTR)o->lCustData == 0 ? "" : (o->Flags & OFN_OVERWRITEPROMPT ? "newfile" : "w16test.exe"));
        PostMessage(dlg, WM_COMMAND, IDOK, 0);
        return TRUE;
    }
    return FALSE;
}

UINT CALLBACK _export OkHook(HWND dlg, UINT msg, WPARAM wp, LPARAM lp)
{
    if (msg == WM_INITDIALOG) {
        hook_inits++;
        PostMessage(dlg, WM_COMMAND, IDOK, 0);
        return TRUE;
    }
    return FALSE;
}

UINT CALLBACK _export FontHook(HWND dlg, UINT msg, WPARAM wp, LPARAM lp)
{
    if (msg == WM_INITDIALOG) {
        LOGFONT lf;
        hook_inits++;
        memset(&lf, 0, sizeof lf);
        SendMessage(dlg, WM_CHOOSEFONT_GETLOGFONT, 0, (LPARAM)(LPLOGFONT)&lf);
        hook_ok = lstrcmp(lf.lfFaceName, "Times New Roman") == 0 && lf.lfWeight == FW_BOLD;
        PostMessage(dlg, WM_COMMAND, IDOK, 0);
        return TRUE;
    }
    return FALSE;
}

UINT CALLBACK _export SetupHook(HWND dlg, UINT msg, WPARAM wp, LPARAM lp)
{
    if (msg == WM_INITDIALOG) {
        hook_inits++;
        hook_ok = IsDlgButtonChecked(dlg, 0x420) && !IsDlgButtonChecked(dlg, 0x421);
        /* Landscape. */
        CheckRadioButton(dlg, 0x420, 0x421, 0x421);
        PostMessage(dlg, WM_COMMAND, IDOK, 0);
        return TRUE;
    }
    return FALSE;
}

static BOOL is_file(LPSTR p, const char *want)
{
    return lstrcmpi(p, want) == 0;
}

static void commdlg_tests(HWND hwnd)
{
    OPENFILENAME ofn;
    CHOOSECOLOR cc;
    CHOOSEFONT cf;
    LOGFONT lf;
    PRINTDLG pd;
    FINDREPLACE fr;
    COLORREF cust[16];
    char file[128], title[20], what[40];
    HWND hdlg;
    int i;

    memset(&ofn, 0, sizeof ofn);
    ofn.lStructSize = sizeof ofn - 1;
    check(!GetOpenFileName(&ofn) && CommDlgExtendedError() == CDERR_STRUCTSIZE, "GetOpenFileName with a bad size: CDERR_STRUCTSIZE");
    check(GetFileTitle("C:\\RS2\\W16TEST.EXE", title, sizeof title) == 0 && lstrcmp(title, "W16TEST.EXE") == 0, "GetFileTitle");

    /* Open: the hook checks the controls, types a name and presses OK. */
    memset(&ofn, 0, sizeof ofn);
    file[0] = 0;
    ofn.lStructSize = sizeof ofn;
    ofn.hwndOwner = hwnd;
    ofn.lpstrFilter = "Programs\0*.exe\0All\0*.*\0";
    ofn.nFilterIndex = 1;
    ofn.lpstrFile = file;
    ofn.nMaxFile = sizeof file;
    ofn.lpstrFileTitle = title;
    ofn.nMaxFileTitle = sizeof title;
    ofn.lpstrInitialDir = "C:\\RS2";
    ofn.Flags = OFN_FILEMUSTEXIST | OFN_HIDEREADONLY | OFN_ENABLEHOOK | OFN_NOCHANGEDIR;
    ofn.lCustData = 1234;
    ofn.lpfnHook = (LPOFNHOOKPROC)MakeProcInstance((FARPROC)FileHook, hInstance);
    hook_inits = hook_ok = 0;
    check(GetOpenFileName(&ofn), "GetOpenFileName (a hook presses OK)");
    check(hook_inits == 1 && hook_ok, "the hook's WM_INITDIALOG: the structure, edt1 lst1 lst2 cmb1 cmb2, files listed");
    check(is_file(file, "c:\\rs2\\w16test.exe") && ofn.nFileOffset == 7 && ofn.nFileExtension == 15, "GetOpenFileName: the path, nFileOffset, nFileExtension");
    check(lstrcmpi(title, "w16test.exe") == 0 && ofn.nFilterIndex == 1, "GetOpenFileName: the file title and filter index");

    /* Save As: the default extension. */
    file[0] = 0;
    ofn.Flags = OFN_OVERWRITEPROMPT | OFN_HIDEREADONLY | OFN_ENABLEHOOK | OFN_NOCHANGEDIR;
    ofn.lpstrDefExt = "txt";
    check(GetSaveFileName(&ofn) && is_file(file, "c:\\rs2\\newfile.txt") && (ofn.Flags & OFN_EXTENSIONDIFFERENT) == 0,
          "GetSaveFileName: the default extension added");
    /* Too small a buffer. */
    file[0] = 0;
    ofn.nMaxFile = 6;
    ofn.Flags = OFN_HIDEREADONLY | OFN_ENABLEHOOK | OFN_NOCHANGEDIR;
    check(!GetOpenFileName(&ofn) && CommDlgExtendedError() == FNERR_BUFFERTOOSMALL && *(WORD *)file == 19,
          "GetOpenFileName into a small buffer: FNERR_BUFFERTOOSMALL and the size");
    FreeProcInstance((FARPROC)ofn.lpfnHook);

    /* Color */
    memset(&cc, 0, sizeof cc);
    for (i = 0; i < 16; i++)
        cust[i] = RGB(i * 16, 0, 0);
    cc.lStructSize = sizeof cc;
    cc.hwndOwner = hwnd;
    cc.rgbResult = RGB(0, 128, 128);
    cc.lpCustColors = cust;
    cc.Flags = CC_RGBINIT | CC_ENABLEHOOK;
    cc.lpfnHook = (LPCCHOOKPROC)MakeProcInstance((FARPROC)OkHook, hInstance);
    hook_inits = 0;
    check(ChooseColor(&cc) && hook_inits == 1 && cc.rgbResult == RGB(0, 128, 128) && cust[5] == RGB(80, 0, 0), "ChooseColor (OK): the colour and custom colours kept");
    cc.Flags = CC_RGBINIT | CC_FULLOPEN | CC_ENABLEHOOK;
    check(ChooseColor(&cc) && hook_inits == 2, "ChooseColor full open");
    FreeProcInstance((FARPROC)cc.lpfnHook);

    /* Font */
    memset(&lf, 0, sizeof lf);
    lf.lfHeight = -16;
    lf.lfWeight = FW_BOLD;
    lstrcpy(lf.lfFaceName, "Times New Roman");
    memset(&cf, 0, sizeof cf);
    cf.lStructSize = sizeof cf;
    cf.hwndOwner = hwnd;
    cf.lpLogFont = &lf;
    cf.Flags = CF_SCREENFONTS | CF_INITTOLOGFONTSTRUCT | CF_EFFECTS | CF_ENABLEHOOK;
    cf.rgbColors = RGB(255, 0, 0);
    cf.lpfnHook = (LPCFHOOKPROC)MakeProcInstance((FARPROC)FontHook, hInstance);
    hook_inits = hook_ok = 0;
    check(ChooseFont(&cf) && hook_inits == 1 && hook_ok, "ChooseFont: WM_CHOOSEFONT_GETLOGFONT in the hook");
    check(lstrcmp(lf.lfFaceName, "Times New Roman") == 0 && lf.lfWeight == FW_BOLD && cf.iPointSize == 120 && lf.lfHeight == -16,
          "ChooseFont: the LOGFONT and iPointSize (12 points)");
    check(cf.rgbColors == RGB(255, 0, 0) && (cf.nFontType & BOLD_FONTTYPE), "ChooseFont: the colour and font type");
    FreeProcInstance((FARPROC)cf.lpfnHook);

    /* Print: the default printer, its DC. */
    memset(&pd, 0, sizeof pd);
    pd.lStructSize = sizeof pd;
    pd.hwndOwner = hwnd;
    pd.Flags = PD_RETURNDEFAULT | PD_RETURNDC;
    check(PrintDlg(&pd) && pd.hDevNames && pd.hDevMode && pd.hDC, "PrintDlg PD_RETURNDEFAULT: hDevNames, hDevMode, a DC");
    if (pd.hDevNames) {
        DEVNAMES FAR *dn = (DEVNAMES FAR *)GlobalLock(pd.hDevNames);
        char dev[120];
        GetProfileString("windows", "device", "", dev, sizeof dev);
        wsprintf(line, "%s,%s,%s", (LPSTR)dn + dn->wDeviceOffset, (LPSTR)dn + dn->wDriverOffset, (LPSTR)dn + dn->wOutputOffset);
        check(lstrcmp(line, dev) == 0 && dn->wDefault == DN_DEFAULTPRN, "DEVNAMES: WIN.INI's default printer");
        GlobalUnlock(pd.hDevNames);
    }
    if (pd.hDC) {
        check(GetDeviceCaps(pd.hDC, TECHNOLOGY) == DT_RASPRINTER, "PrintDlg's DC is a printer's");
        DeleteDC(pd.hDC);
    }
    /* Setup: the hook chooses Landscape. */
    pd.Flags = PD_PRINTSETUP | PD_ENABLESETUPHOOK;
    pd.lpfnSetupHook = (LPSETUPHOOKPROC)MakeProcInstance((FARPROC)SetupHook, hInstance);
    hook_inits = hook_ok = 0;
    check(PrintDlg(&pd) && hook_inits == 1 && hook_ok, "PrintDlg PD_PRINTSETUP: portrait at first");
    if (pd.hDevMode) {
        DEVMODE FAR *dm = (DEVMODE FAR *)GlobalLock(pd.hDevMode);
        check(dm->dmOrientation == 2, "Print Setup: landscape in the DEVMODE");
        GlobalUnlock(pd.hDevMode);
    }
    FreeProcInstance((FARPROC)pd.lpfnSetupHook);
    /* Print with the hook: OK, a DC with the landscape page; the copies
     * are the DEVMODE's. */
    if (pd.hDevMode) {
        DEVMODE FAR *dm = (DEVMODE FAR *)GlobalLock(pd.hDevMode);
        dm->dmCopies = 2;
        GlobalUnlock(pd.hDevMode);
    }
    pd.Flags = PD_RETURNDC | PD_ENABLEPRINTHOOK | PD_NOSELECTION;
    pd.nCopies = 1;
    pd.lpfnPrintHook = (LPPRINTHOOKPROC)MakeProcInstance((FARPROC)OkHook, hInstance);
    hook_inits = 0;
    i = PrintDlg(&pd);
    wsprintf(line, "PrintDlg (OK): copies, a DC (%d %d %d %04x %lx)", i, hook_inits, pd.nCopies, pd.hDC, CommDlgExtendedError());
    check(i && hook_inits == 1 && pd.nCopies == 2 && pd.hDC, line);
    if (pd.hDC) {
        check(GetDeviceCaps(pd.hDC, HORZRES) > GetDeviceCaps(pd.hDC, VERTRES), "the DC's page is landscape (from the DEVMODE)");
        DeleteDC(pd.hDC);
    }
    FreeProcInstance((FARPROC)pd.lpfnPrintHook);
    GlobalFree(pd.hDevMode);
    GlobalFree(pd.hDevNames);

    /* Find: modeless; FINDMSGSTRING to the owner. */
    memset(&fr, 0, sizeof fr);
    lstrcpy(what, "needle");
    fr.lStructSize = sizeof fr;
    fr.hwndOwner = hwnd;
    fr.Flags = FR_DOWN | FR_MATCHCASE;
    fr.lpstrFindWhat = what;
    fr.wFindWhatLen = sizeof what;
    hdlg = FindText(&fr);
    check(hdlg != 0 && IsWindow(hdlg), "FindText: a modeless dialog");
    if (hdlg) {
        char t[40];
        GetDlgItemText(hdlg, 0x480, t, sizeof t);
        check(lstrcmp(t, "needle") == 0 && IsDlgButtonChecked(hdlg, 0x411) && IsDlgButtonChecked(hdlg, 0x421), "FindText: the text, Match Case, Down");
        SetDlgItemText(hdlg, 0x480, "haystack");
        findmsgs = 0;
        SendMessage(hdlg, WM_COMMAND, IDOK, 0);
        check(findmsgs == 1 && (find_flags & FR_FINDNEXT) && (find_flags & FR_MATCHCASE) && lstrcmp(find_seen, "haystack") == 0,
              "Find Next: FINDMSGSTRING with FR_FINDNEXT and the text");
        SendMessage(hdlg, WM_COMMAND, IDCANCEL, 0);
        check(findmsgs == 2 && (find_flags & FR_DIALOGTERM) && !IsWindow(hdlg), "Cancel: FR_DIALOGTERM, the dialog gone");
    }
    fr.lpstrReplaceWith = file;
    fr.wReplaceWithLen = sizeof file;
    lstrcpy(file, "pin");
    hdlg = ReplaceText(&fr);
    check(hdlg != 0, "ReplaceText");
    if (hdlg) {
        findmsgs = 0;
        SendMessage(hdlg, WM_COMMAND, 0x401, 0);
        check(findmsgs == 1 && (find_flags & FR_REPLACEALL) && lstrcmp(file, "pin") == 0, "Replace All: FR_REPLACEALL");
        DestroyWindow(hdlg);
    }
}

BOOL FAR PASCAL _export TestAbortProc(HDC hdc, int code)
{
    return TRUE;
}

static void print_tests(void)
{
    char dev[120], *device, *driver, *port;
    HDC dc, ic;
    DOCINFO di;
    int esc = NEWFRAME;

    GetProfileString("windows", "device", "", dev, sizeof dev);
    device = strtok(dev, ",");
    driver = strtok(NULL, ",");
    port = strtok(NULL, ",");
    check(device && driver && port, "WIN.INI [windows] device=");
    if (!device || !driver || !port)
        return;
    GetProfileString("devices", device, "", line, sizeof line);
    check(_fstrncmp(line, driver, lstrlen(driver)) == 0, "WIN.INI [devices]: the driver and port");
    ic = CreateIC(driver, device, port, NULL);
    wsprintf(line, "CreateIC on the printer: 300 dpi (%04x, %d)", ic, ic ? GetDeviceCaps(ic, LOGPIXELSY) : 0);
    check(ic && GetDeviceCaps(ic, LOGPIXELSY) == 300, line);
    DeleteDC(ic);
    dc = CreateDC(driver, device, port, NULL);
    check(dc != 0, "CreateDC on the printer");
    if (!dc)
        return;
    check(Escape(dc, QUERYESCSUPPORT, sizeof esc, (LPSTR)&esc, NULL) > 0, "Escape QUERYESCSUPPORT");
    SetAbortProc(dc, (ABORTPROC)MakeProcInstance((FARPROC)TestAbortProc, hInstance));
    di.cbSize = sizeof di;
    di.lpszDocName = "W16TEST";
    di.lpszOutput = NULL;
    check(StartDoc(dc, &di) > 0 && StartPage(dc) > 0, "StartDoc, StartPage");
    TextOut(dc, 300, 300, "W16TEST", 7);
    Rectangle(dc, 300, 400, 900, 700);
    check(EndPage(dc) > 0 && EndDoc(dc) > 0, "EndPage, EndDoc");
    check(StartDoc(dc, &di) > 0 && AbortDoc(dc) > 0, "AbortDoc");
    check(DeleteDC(dc), "DeleteDC");
}

static void stage3_atom_tests(void)
{
    ATOM a = GlobalAddAtom("W16TEST atom"), b;
    char t[40];
    check(a >= 0xc000 && GlobalFindAtom("w16test ATOM") == a, "GlobalAddAtom, GlobalFindAtom (case blind)");
    check(GlobalGetAtomName(a, t, sizeof t) == 12 && lstrcmp(t, "W16TEST atom") == 0, "GlobalGetAtomName");
    GlobalDeleteAtom(a);
    check(GlobalFindAtom("W16TEST atom") == 0, "GlobalDeleteAtom");
    b = AddAtom("local");
    check(b >= 0xc000 && FindAtom("LOCAL") == b && GetAtomName(b, t, sizeof t) == 5, "AddAtom, FindAtom, GetAtomName");
    check(AddAtom(MAKEINTATOM(42)) == 42 && GetAtomName(42, t, sizeof t) && lstrcmp(t, "#42") == 0, "integer atoms");
    check(RegisterWindowMessage("W16TEST msg") == RegisterWindowMessage("w16test MSG") && RegisterWindowMessage("W16TEST msg") >= 0xc000,
          "RegisterWindowMessage: one number per name");
}

static HGLOBAL global_with(const void FAR *p, int n)
{
    HGLOBAL h = GlobalAlloc(GMEM_MOVEABLE | GMEM_DDESHARE, n);
    _fmemcpy(GlobalLock(h), p, n);
    GlobalUnlock(h);
    return h;
}

static void clipboard_tests(HWND hwnd)
{
    static const BYTE dib[40 + 16] = {40, 0, 0, 0, 2, 0, 0, 0, 2, 0, 0, 0, 1, 0, 24, 0, 0, 0, 0, 0, 16, 0, 0, 0,
                                      0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                                      0, 0, 255, 0, 255, 0, 0, 0, 255, 0, 0, 0, 0, 255, 0, 0};
    METAFILEPICT mp, FAR *pm;
    HGLOBAL h;
    BYTE FAR *p;
    UINT f;
    int n, dibs = 0, bitmaps = 0;
    char t[40];

    reg_fmt = RegisterClipboardFormat("W16TEST format");
    check(reg_fmt >= 0xc000 && RegisterClipboardFormat("W16TEST format") == reg_fmt, "RegisterClipboardFormat");
    check(GetClipboardFormatName(reg_fmt, t, sizeof t) == 14 && lstrcmp(t, "W16TEST format") == 0, "GetClipboardFormatName");
    check(OpenClipboard(hwnd) && EmptyClipboard(), "OpenClipboard, EmptyClipboard");
    SetClipboardData(CF_TEXT, global_with("clip text", 10));
    SetClipboardData(reg_fmt, global_with("0123456789", 10));
    SetClipboardData(CF_DIB, global_with(dib, sizeof dib));
    mp.mm = MM_ANISOTROPIC;
    mp.xExt = 100;
    mp.yExt = 50;
    mp.hMF = global_with("metafile bits here!", 20);
    SetClipboardData(CF_METAFILEPICT, global_with(&mp, sizeof mp));
    /* Delayed rendering of a second registered format. */
    SetClipboardData(reg_fmt + 0, global_with("0123456789", 10));
    SetClipboardData(RegisterClipboardFormat("W16TEST delayed"), NULL);
    CloseClipboard();

    check(IsClipboardFormatAvailable(CF_TEXT) && IsClipboardFormatAvailable(CF_OEMTEXT) && IsClipboardFormatAvailable(reg_fmt) &&
          IsClipboardFormatAvailable(CF_DIB) && IsClipboardFormatAvailable(CF_METAFILEPICT), "IsClipboardFormatAvailable");
    check(IsClipboardFormatAvailable(CF_BITMAP), "a DIB is a bitmap for PM programs too");
    OpenClipboard(hwnd);
    for (f = 0, n = 0; (f = EnumClipboardFormats(f)) != 0 && n < 20; n++) {
        dibs += f == CF_DIB;
        bitmaps += f == CF_BITMAP;
    }
    check(n == CountClipboardFormats() && n >= 6 && dibs == 1, "EnumClipboardFormats, CountClipboardFormats");
    h = GetClipboardData(CF_TEXT);
    check(h && lstrcmp(GlobalLock(h), "clip text") == 0, "GetClipboardData CF_TEXT");
    h = GetClipboardData(reg_fmt);
    p = h ? GlobalLock(h) : NULL;
    check(p && GlobalSize(h) == 10 && _fmemcmp(p, "0123456789", 10) == 0, "a registered format: the bytes and GlobalSize");
    h = GetClipboardData(CF_DIB);
    p = h ? GlobalLock(h) : NULL;
    check(p && GlobalSize(h) == sizeof dib && _fmemcmp(p, dib, sizeof dib) == 0, "GetClipboardData CF_DIB");
    h = GetClipboardData(CF_METAFILEPICT);
    pm = h ? (METAFILEPICT FAR *)GlobalLock(h) : NULL;
    check(pm && pm->mm == MM_ANISOTROPIC && pm->xExt == 100 && pm->yExt == 50 && pm->hMF &&
          lstrcmp(GlobalLock(pm->hMF), "metafile bits here!") == 0, "GetClipboardData CF_METAFILEPICT (with the metafile)");
    renders = 0;
    h = GetClipboardData(RegisterClipboardFormat("W16TEST delayed"));
    check(renders == 1 && h && lstrcmp(GlobalLock(h), "rendered") == 0, "delayed rendering: WM_RENDERFORMAT to the owner");
    check(GetClipboardOwner() == hwnd, "GetClipboardOwner");
    destroys = 0;
    EmptyClipboard();
    CloseClipboard();
    check(destroys == 1 && !IsClipboardFormatAvailable(CF_TEXT), "EmptyClipboard: WM_DESTROYCLIPBOARD to the owner");

    /* CF_BITMAP: a GDI bitmap; it comes back as a bitmap and a DIB. */
    {
        HDC dc = GetDC(hwnd), mem = CreateCompatibleDC(dc);
        HBITMAP bm = CreateCompatibleBitmap(dc, 5, 3), got, old;
        BITMAP info;
        RECT rc;
        old = SelectObject(mem, bm);
        SetRect(&rc, 0, 0, 5, 3);
        FillRect(mem, &rc, GetStockObject(BLACK_BRUSH));
        SelectObject(mem, old);
        DeleteDC(mem);
        ReleaseDC(hwnd, dc);
        OpenClipboard(hwnd);
        EmptyClipboard();
        check(SetClipboardData(CF_BITMAP, bm) == bm, "SetClipboardData CF_BITMAP");
        CloseClipboard();
        check(IsClipboardFormatAvailable(CF_BITMAP) && IsClipboardFormatAvailable(CF_DIB), "a bitmap is a DIB too");
        OpenClipboard(hwnd);
        got = GetClipboardData(CF_BITMAP);
        check(got && GetObject(got, sizeof info, &info) && info.bmWidth == 5 && info.bmHeight == 3, "GetClipboardData CF_BITMAP: the bitmap's size");
        h = GetClipboardData(CF_DIB);
        p = h ? GlobalLock(h) : NULL;
        check(p && ((BITMAPINFOHEADER FAR *)p)->biWidth == 5 && ((BITMAPINFOHEADER FAR *)p)->biHeight == 3, "GetClipboardData CF_DIB of a bitmap");
        EmptyClipboard();
        CloseClipboard();
    }
}

/* ---- DDEML in one task: a server and a client instance ---- */

static DWORD srv, cli;
static HSZ hszSvc, hszTopic, hszItem;
static char srv_text[40] = "initial";
static int advdata, srv_connects;
static char adv_seen[40];

HDDEDATA CALLBACK _export SrvCallback(UINT type, UINT fmt, HCONV conv, HSZ h1, HSZ h2, HDDEDATA data, DWORD d1, DWORD d2)
{
    switch (type) {
    case XTYP_CONNECT:
        srv_connects++;
        return (HDDEDATA)(DdeCmpStringHandles(h1, hszTopic) == 0);
    case XTYP_REQUEST:
    case XTYP_ADVREQ:
        return DdeCreateDataHandle(srv, srv_text, lstrlen(srv_text) + 1, 0, h2, CF_TEXT, 0);
    case XTYP_POKE:
        DdeGetData(data, srv_text, sizeof srv_text, 0);
        DdePostAdvise(srv, hszTopic, hszItem);
        return (HDDEDATA)DDE_FACK;
    case XTYP_EXECUTE: {
        char cmd[40];
        DdeGetData(data, cmd, sizeof cmd, 0);
        return (HDDEDATA)(lstrcmp(cmd, "[ok]") == 0 ? DDE_FACK : DDE_FNOTPROCESSED);
    }
    case XTYP_ADVSTART:
        return (HDDEDATA)TRUE;
    }
    return 0;
}

HDDEDATA CALLBACK _export CliCallback(UINT type, UINT fmt, HCONV conv, HSZ h1, HSZ h2, HDDEDATA data, DWORD d1, DWORD d2)
{
    if (type == XTYP_ADVDATA) {
        advdata++;
        DdeGetData(data, adv_seen, sizeof adv_seen, 0);
        return (HDDEDATA)DDE_FACK;
    }
    return 0;
}

static void ddeml_tests(void)
{
    HCONV conv;
    HDDEDATA d;
    DWORD res, n;
    char t[40];
    LPBYTE p;

    check(DdeInitialize(&srv, (PFNCALLBACK)MakeProcInstance((FARPROC)SrvCallback, hInstance), APPCLASS_STANDARD, 0) == DMLERR_NO_ERROR,
          "DdeInitialize (server)");
    check(DdeInitialize(&cli, (PFNCALLBACK)MakeProcInstance((FARPROC)CliCallback, hInstance), APPCMD_CLIENTONLY, 0) == DMLERR_NO_ERROR &&
          cli != srv, "DdeInitialize (client)");
    hszSvc = DdeCreateStringHandle(srv, "W16TestSvc", CP_WINANSI);
    hszTopic = DdeCreateStringHandle(srv, "Topic", CP_WINANSI);
    hszItem = DdeCreateStringHandle(srv, "Item", CP_WINANSI);
    check(hszSvc && DdeQueryString(srv, hszSvc, t, sizeof t, CP_WINANSI) == 10 && lstrcmp(t, "W16TestSvc") == 0, "DdeCreateStringHandle, DdeQueryString");
    check(DdeCmpStringHandles(hszSvc, DdeCreateStringHandle(cli, "w16testsvc", CP_WINANSI)) == 0, "DdeCmpStringHandles (case blind)");
    d = DdeCreateDataHandle(cli, "abc", 4, 0, 0, CF_TEXT, 0);
    d = DdeAddData(d, "xyz", 4, 3);
    p = DdeAccessData(d, &n);
    check(p && n == 7 && lstrcmp((LPSTR)p, "abcxyz") == 0, "DdeCreateDataHandle, DdeAddData, DdeAccessData");
    DdeUnaccessData(d);
    check(DdeGetData(d, NULL, 0, 0) == 7 && DdeFreeDataHandle(d), "DdeGetData (size), DdeFreeDataHandle");

    check(DdeConnect(cli, hszSvc, hszTopic, NULL) == 0 && DdeGetLastError(cli) == DMLERR_NO_CONV_ESTABLISHED, "DdeConnect before DdeNameService: none");
    DdeNameService(srv, hszSvc, 0, DNS_REGISTER);
    conv = DdeConnect(cli, hszSvc, hszTopic, NULL);
    check(conv != 0 && srv_connects >= 1, "DdeConnect: XTYP_CONNECT, a conversation");
    if (!conv)
        return;
    d = DdeClientTransaction(NULL, 0, conv, hszItem, CF_TEXT, XTYP_REQUEST, 5000, &res);
    check(d && DdeGetData(d, t, sizeof t, 0) == 8 && lstrcmp(t, "initial") == 0, "XTYP_REQUEST");
    DdeFreeDataHandle(d);
    check(DdeClientTransaction(NULL, 0, conv, hszItem, CF_TEXT, XTYP_ADVSTART, 5000, &res) != 0, "XTYP_ADVSTART");
    check(DdeClientTransaction("poked", 6, conv, hszItem, CF_TEXT, XTYP_POKE, 5000, &res) && (res & DDE_FACK), "XTYP_POKE: DDE_FACK");
    /* The advise data comes in a posted message. */
    {
        MSG msg;
        DWORD start = GetTickCount();
        while (!advdata && GetTickCount() - start < 3000)
            if (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE))
                DispatchMessage(&msg);
    }
    check(advdata == 1 && lstrcmp(adv_seen, "poked") == 0, "DdePostAdvise: XTYP_ADVDATA with the new value");
    check(DdeClientTransaction("[ok]", 5, conv, 0, 0, XTYP_EXECUTE, 5000, &res) && (res & DDE_FACK), "XTYP_EXECUTE");
    check(!DdeClientTransaction("[no]", 5, conv, 0, 0, XTYP_EXECUTE, 5000, &res) && DdeGetLastError(cli) == DMLERR_NOTPROCESSED,
          "XTYP_EXECUTE refused: DMLERR_NOTPROCESSED");
    check(DdeClientTransaction(NULL, 0, conv, hszItem, CF_TEXT, XTYP_ADVSTOP, 5000, &res) != 0, "XTYP_ADVSTOP");
    {
        CONVINFO ci;
        ci.cb = sizeof ci;
        check(DdeQueryConvInfo(conv, QID_SYNC, &ci) && (ci.wStatus & ST_CONNECTED) && (ci.wStatus & ST_CLIENT) && ci.hszTopic == hszTopic,
              "DdeQueryConvInfo");
    }
    check(DdeDisconnect(conv), "DdeDisconnect");
    check(DdeUninitialize(cli) && DdeUninitialize(srv), "DdeUninitialize");
}

/* A PM program's DDE server (PMXTEST DDESERVER): what came back. */
static HWND pm_server;
static int pm_acks, pm_datas, pm_terms;
static WORD pm_status;
static HGLOBAL pm_hi;
static char pm_data[60];

static void pump(int *n, int was)
{
    DWORD start = GetTickCount();
    MSG msg;
    while (*n == was && GetTickCount() - start < 5000)
        if (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE))
            DispatchMessage(&msg);
}

/* Until the PM server's ACK of one message: its item's name, or an
 * execute's commands (h). Another WM_DDE_ACK that comes first is not
 * this message's. How many ACKs came, 0 if its did not. */
static int pump_ack(const char *item, HGLOBAL h)
{
    DWORD start = GetTickCount();
    MSG msg;
    char name[16];
    int n = pm_acks, got = 0;

    while (GetTickCount() - start < 5000) {
        if (pm_acks != n) {
            n = pm_acks;
            got++;
            if (h ? pm_hi == h : GlobalGetAtomName((ATOM)pm_hi, name, sizeof name) && lstrcmpi(name, item) == 0)
                return got;
        }
        if (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE))
            DispatchMessage(&msg);
    }
    return 0;
}

static void raw_dde_tests(HWND hwnd)
{
    /* WM_DDE_INITIATE broadcast reaches the task's own windows. */
    ATOM a = GlobalAddAtom("NoSuchService"), app, topic;
    LRESULT r = SendMessage((HWND)0xffff, WM_DDE_INITIATE, (WPARAM)hwnd, MAKELONG(a, a));
    HGLOBAL h;
    WORD FAR *p;
    int i, n;
    check(r == 0, "WM_DDE_INITIATE to all windows (nobody answers)");
    GlobalDeleteAtom(a);

    /* A PM program's server, through PM's DDE. */
    check(WinExec("C:\\RS2\\PMXTEST.EXE DDESERVER", SW_SHOWNORMAL) > 32, "WinExec PMXTEST DDESERVER (a PM DDE server)");
    app = GlobalAddAtom("PMXTEST");
    topic = GlobalAddAtom("System");
    for (i = 0; i < 40 && !pm_server; i++) {
        SendMessage((HWND)0xffff, WM_DDE_INITIATE, (WPARAM)hwnd, MAKELONG(app, topic));
        if (!pm_server) {
            int never = 0;
            DWORD start = GetTickCount();
            while (GetTickCount() - start < 250)
                pump(&never, 0);
        }
    }
    check(pm_server != 0, "WM_DDE_INITIATE reaches a PM server: WM_DDE_ACK (its WM_DDE_INITIATEACK)");
    if (!pm_server)
        return;
    n = pm_datas;
    PostMessage(pm_server, WM_DDE_REQUEST, (WPARAM)hwnd, MAKELONG(CF_TEXT, GlobalAddAtom("Greeting")));
    pump(&pm_datas, n);
    check(pm_datas == n + 1 && lstrcmp(pm_data, "hello from the server") == 0, "WM_DDE_REQUEST to the PM server: WM_DDE_DATA (from its DDESTRUCT)");
    h = GlobalAlloc(GMEM_MOVEABLE | GMEM_DDESHARE, 4 + 6);
    p = (WORD FAR *)GlobalLock(h);
    p[0] = 0x2000;
    p[1] = CF_TEXT;
    _fmemcpy(p + 2, "12345", 6);
    GlobalUnlock(h);
    PostMessage(pm_server, WM_DDE_POKE, (WPARAM)hwnd, MAKELONG(h, GlobalAddAtom("Value")));
    check(pump_ack("Value", 0) && (pm_status & 0x8000), "WM_DDE_POKE to the PM server: a positive WM_DDE_ACK");
    h = GlobalAlloc(GMEM_MOVEABLE | GMEM_DDESHARE, 7);
    lstrcpy(GlobalLock(h), "[quit]");
    GlobalUnlock(h);
    PostMessage(pm_server, WM_DDE_EXECUTE, (WPARAM)hwnd, MAKELONG(0, h));
    n = pump_ack(NULL, h);
    wsprintf(line, "WM_DDE_EXECUTE to the PM server: ACK with the commands back (%d %04x %04x %04x)", n, pm_status, pm_hi, h);
    check(n && (pm_status & 0x8000) && pm_hi == h, line);
    GlobalFree(h);
    n = pm_terms;
    PostMessage(pm_server, WM_DDE_TERMINATE, (WPARAM)hwnd, 0);
    pump(&pm_terms, n);
    check(pm_terms == n + 1, "WM_DDE_TERMINATE: the PM server terminates too");
    GlobalDeleteAtom(app);
    GlobalDeleteAtom(topic);
}

/* CreateFont on the screen: Courier New is fixed pitch. */
static void font_tests(HWND hwnd)
{
    HDC dc = GetDC(hwnd);
    HFONT f = CreateFont(-13, 0, 0, 0, FW_BOLD, 0, 0, 0, ANSI_CHARSET, 0, 0, 0, FIXED_PITCH | FF_MODERN, "Courier New");
    HFONT old = SelectObject(dc, f);
    DWORD i = GetTextExtent(dc, "iiii", 4), w = GetTextExtent(dc, "WWWW", 4);
    char face[40];
    GetTextFace(dc, sizeof face, face);
    wsprintf(line, "CreateFont Courier New: fixed pitch (%d, %d, %s)", LOWORD(i), LOWORD(w), (LPSTR)face);
    check(LOWORD(i) == LOWORD(w), line);
    SelectObject(dc, old);
    DeleteObject(f);
    ReleaseDC(hwnd, dc);
}

LONG FAR PASCAL _export Stage3WndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    if (msg == findmsg && findmsg) {
        FINDREPLACE FAR *f = (FINDREPLACE FAR *)lp;
        findmsgs++;
        find_flags = f->Flags;
        lstrcpy(find_seen, f->lpstrFindWhat);
        return 0;
    }
    switch (msg) {
    case WM_DDE_ACK:
        if (!pm_server) {
            pm_server = (HWND)wp;
            GlobalDeleteAtom(LOWORD(lp));
            GlobalDeleteAtom(HIWORD(lp));
        } else {
            pm_status = LOWORD(lp);
            pm_hi = (HGLOBAL)HIWORD(lp);
            pm_acks++;
        }
        return 0;
    case WM_DDE_DATA: {
        HGLOBAL h = (HGLOBAL)LOWORD(lp);
        WORD FAR *p = (WORD FAR *)GlobalLock(h);
        lstrcpy(pm_data, (LPSTR)(p + 2));
        GlobalUnlock(h);
        GlobalFree(h);
        GlobalDeleteAtom(HIWORD(lp));
        pm_datas++;
        return 0;
    }
    case WM_DDE_TERMINATE:
        pm_terms++;
        return 0;
    case WM_RENDERFORMAT:
        renders++;
        SetClipboardData(wp, global_with("rendered", 9));
        return 0;
    case WM_DESTROYCLIPBOARD:
        destroys++;
        return 0;
    }
    return DefWindowProc(hwnd, msg, wp, lp);
}

static void stage3_tests(HINSTANCE hInst)
{
    WNDCLASS wc;

    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = Stage3WndProc;
    wc.hInstance = hInst;
    wc.hCursor = LoadCursor(NULL, IDC_ARROW);
    wc.hbrBackground = GetStockObject(WHITE_BRUSH);
    wc.lpszClassName = "Stage3";
    RegisterClass(&wc);
    findmsg = RegisterWindowMessage("commdlg_FindReplace");
    s3wnd = CreateWindow("Stage3", "W16TEST stage 3", WS_OVERLAPPEDWINDOW | WS_VISIBLE, 40, 40, 400, 300,
                         NULL, NULL, hInst, NULL);
    font_tests(s3wnd);
    stage3_atom_tests();
    clipboard_tests(s3wnd);
    print_tests();
    commdlg_tests(s3wnd);
    ddeml_tests();
    raw_dde_tests(s3wnd);
    DestroyWindow(s3wnd);
}

/* ---- What real programs needed (docs/WIN16.md, "Real programs") ---- */

static void bitmap_tests(void)
{
    HDC mem, mem2;
    HBITMAP bm, bm2, mono, old, old2;
    BITMAP info;
    static BYTE bits[8 * 2] = { 0xff, 0, 0x00, 0, 0xf0, 0, 0x0f, 0, 0xff, 0, 0, 0, 0, 0, 0, 0 };
    BYTE back[16];
    struct { BITMAPINFOHEADER h; RGBQUAD c[2]; } bi;
    BYTE row[64];

    mem = CreateCompatibleDC(NULL);
    check(mem != 0, "CreateCompatibleDC");
    bm = CreateCompatibleBitmap(mem, 16, 16);
    check(bm != 0 && GetObject(bm, sizeof info, &info) == sizeof info && info.bmWidth == 16 && info.bmBitsPixel == 1,
          "CreateCompatibleBitmap of a new memory DC is monochrome");
    DeleteObject(bm);
    bm = CreateBitmap(16, 16, 1, 24, NULL);
    check(GetObject(bm, sizeof info, &info) == sizeof info && info.bmHeight == 16 && info.bmBitsPixel == 24, "CreateBitmap 24 bpp, GetObject");
    old = SelectObject(mem, bm);
    check(old != 0, "SelectObject of a bitmap into a memory DC");
    PatBlt(mem, 0, 0, 16, 16, WHITENESS);
    check(GetPixel(mem, 5, 5) == RGB(255, 255, 255), "PatBlt WHITENESS, GetPixel");
    SetPixel(mem, 3, 4, RGB(255, 0, 0));
    check(GetPixel(mem, 3, 4) == RGB(255, 0, 0) && GetPixel(mem, 4, 3) == RGB(255, 255, 255), "SetPixel/GetPixel in a bitmap (y down)");

    mem2 = CreateCompatibleDC(mem);
    bm2 = CreateBitmap(16, 16, 1, 24, NULL);
    old2 = SelectObject(mem2, bm2);
    PatBlt(mem2, 0, 0, 16, 16, BLACKNESS);
    BitBlt(mem2, 8, 8, 8, 8, mem, 0, 0, SRCCOPY);
    check(GetPixel(mem2, 11, 12) == RGB(255, 0, 0) && GetPixel(mem2, 1, 1) == RGB(0, 0, 0), "BitBlt SRCCOPY between memory DCs");
    StretchBlt(mem2, 0, 0, 16, 16, mem, 0, 0, 8, 8, SRCCOPY);
    check(GetPixel(mem2, 6, 8) == RGB(255, 0, 0) && GetPixel(mem2, 7, 9) == RGB(255, 0, 0), "StretchBlt doubles");

    /* A monochrome bitmap: 1 bits take the target's background colour, 0
       bits its text colour. */
    mono = CreateBitmap(8, 8, 1, 1, bits);
    SelectObject(mem2, mono);
    check(GetPixel(mem2, 0, 0) == RGB(255, 255, 255) && GetPixel(mem2, 0, 1) == RGB(0, 0, 0) && GetPixel(mem2, 4, 2) == RGB(0, 0, 0)
          && GetPixel(mem2, 3, 2) == RGB(255, 255, 255), "CreateBitmap monochrome with DDB bits (rows top-down, word aligned)");
    check(GetBitmapBits(mono, sizeof back, back) == 16 && memcmp(back, bits, 16) == 0, "GetBitmapBits");
    SetTextColor(mem, RGB(0, 0, 255));
    SetBkColor(mem, RGB(0, 255, 0));
    BitBlt(mem, 0, 0, 8, 8, mem2, 0, 0, SRCCOPY);
    check(GetPixel(mem, 0, 0) == RGB(0, 255, 0) && GetPixel(mem, 0, 1) == RGB(0, 0, 255), "BitBlt monochrome to colour: bk and text colours");

    /* GetDIBits: 24-bit rows bottom-up. */
    memset(&bi, 0, sizeof bi);
    bi.h.biSize = sizeof bi.h;
    bi.h.biWidth = 16;
    bi.h.biHeight = 16;
    bi.h.biPlanes = 1;
    bi.h.biBitCount = 24;
    SelectObject(mem, old);
    check(GetDIBits(mem, bm, 15, 1, row, (BITMAPINFO FAR *)&bi, DIB_RGB_COLORS) == 1 && row[0] == 0 && row[1] == 255 && row[2] == 0,
          "GetDIBits (the top row is the last scan)");
    row[0] = 0x10; row[1] = 0x20; row[2] = 0x30;
    SetDIBits(mem, bm, 15, 1, row, (BITMAPINFO FAR *)&bi, DIB_RGB_COLORS);
    SelectObject(mem, bm);
    check(GetPixel(mem, 0, 0) == RGB(0x30, 0x20, 0x10), "SetDIBits");
    SelectObject(mem, old);
    SelectObject(mem2, old2);
    check(DeleteObject(bm) && DeleteObject(bm2) && DeleteObject(mono), "DeleteObject of bitmaps");
    DeleteDC(mem2);
    check(DeleteDC(mem), "DeleteDC");
}

static void mapping_tests(void)
{
    HDC dc = CreateCompatibleDC(NULL);
    POINT p;

    SetMapMode(dc, MM_ANISOTROPIC);
    SetWindowExt(dc, 100, 100);
    SetViewportExt(dc, 200, 300);
    SetViewportOrg(dc, 5, 7);
    p.x = 10;
    p.y = 10;
    LPtoDP(dc, &p, 1);
    check(p.x == 25 && p.y == 37, "MM_ANISOTROPIC: LPtoDP");
    DPtoLP(dc, &p, 1);
    check(p.x == 10 && p.y == 10, "DPtoLP");
    check(GetMapMode(dc) == MM_ANISOTROPIC, "GetMapMode");
    SetMapMode(dc, MM_TEXT);
    SetWindowOrg(dc, 3, 4);
    p.x = 3;
    p.y = 4;
    LPtoDP(dc, &p, 1);
    check(p.x == 5 && p.y == 7, "MM_TEXT with a window and viewport origin");
    DeleteDC(dc);
}

static void region_tests(void)
{
    HRGN a = CreateRectRgn(0, 0, 10, 10), b = CreateRectRgn(5, 5, 15, 15), c = CreateRectRgn(0, 0, 0, 0);
    HRGN e;
    RECT r;

    check(CombineRgn(c, a, b, RGN_OR) == COMPLEXREGION, "CombineRgn RGN_OR");
    check(PtInRegion(c, 12, 12) && PtInRegion(c, 2, 2) && !PtInRegion(c, 12, 2), "PtInRegion");
    check(CombineRgn(c, a, b, RGN_AND) == SIMPLEREGION && GetRgnBox(c, &r) && r.left == 5 && r.top == 5 && r.right == 10 && r.bottom == 10,
          "CombineRgn RGN_AND, GetRgnBox");
    check(CombineRgn(c, a, b, RGN_DIFF) == COMPLEXREGION && !PtInRegion(c, 7, 7) && PtInRegion(c, 7, 2), "CombineRgn RGN_DIFF");
    e = CreateEllipticRgn(0, 0, 20, 10);
    check(PtInRegion(e, 10, 5) && !PtInRegion(e, 0, 0), "CreateEllipticRgn");
    OffsetRgn(a, 5, 5);
    check(EqualRgn(a, b), "OffsetRgn, EqualRgn");
    DeleteObject(a);
    DeleteObject(b);
    DeleteObject(c);
    DeleteObject(e);
}

static int atom_tests(HWND hwnd)
{
    ATOM at = AddAtom("W16Test atom");
    ATOM g = GlobalAddAtom("W16Test global");
    char buf[32];

    check(at >= 0xc000 && FindAtom("w16test ATOM") == at, "AddAtom/FindAtom (case does not matter)");
    check(GetAtomName(at, buf, sizeof buf) == 12 && strcmp(buf, "W16Test atom") == 0, "GetAtomName");
    check(AddAtom("#123") == 123, "AddAtom of \"#123\" is the integer atom");
    check(g >= 0xc000 && GlobalFindAtom("W16Test global") == g, "GlobalAddAtom/GlobalFindAtom");
    GlobalDeleteAtom(g);
    check(RegisterWindowMessage("W16TestMessage") >= 0xc000 && RegisterWindowMessage("W16TestMessage") == RegisterWindowMessage("w16testmessage"),
          "RegisterWindowMessage");
    check(SetProp(hwnd, "W16Prop", (HANDLE)0x1234) && GetProp(hwnd, "W16Prop") == (HANDLE)0x1234, "SetProp/GetProp");
    check(RemoveProp(hwnd, "W16Prop") == (HANDLE)0x1234 && GetProp(hwnd, "W16Prop") == 0, "RemoveProp");
    return 0;
}

static void profile_tests(void)
{
    char buf[64];

    check(WritePrivateProfileString("Sect", "Key", "Value one", "W16TEST.INI"), "WritePrivateProfileString");
    WritePrivateProfileString("Sect", "Number", "42", "W16TEST.INI");
    WritePrivateProfileString("Other", "A", "b", "W16TEST.INI");
    check(GetPrivateProfileString("sect", "key", "x", buf, sizeof buf, "W16TEST.INI") == 9 && strcmp(buf, "Value one") == 0,
          "GetPrivateProfileString");
    check(GetPrivateProfileInt("Sect", "Number", 7, "W16TEST.INI") == 42 && GetPrivateProfileInt("Sect", "None", 7, "W16TEST.INI") == 7,
          "GetPrivateProfileInt");
    GetPrivateProfileString("Sect", NULL, "", buf, sizeof buf, "W16TEST.INI");
    check(strcmp(buf, "Key") == 0 && strcmp(buf + 4, "Number") == 0 && buf[11] == 0, "GetPrivateProfileString of a section's keys");
    WritePrivateProfileString("Sect", "Key", NULL, "W16TEST.INI");
    check(GetPrivateProfileString("Sect", "Key", "gone", buf, sizeof buf, "W16TEST.INI") == 4, "WritePrivateProfileString deletes a key");
    WritePrivateProfileString("Sect", NULL, NULL, "W16TEST.INI");
    WritePrivateProfileString("Other", NULL, NULL, "W16TEST.INI");
    check(GetPrivateProfileInt("Sect", "Number", 7, "W16TEST.INI") == 7, "WritePrivateProfileString deletes a section");
    check(WriteProfileString("W16Test", "Key", "win") && GetProfileString("W16Test", "Key", "", buf, sizeof buf) == 3, "WIN.INI");
    WriteProfileString("W16Test", NULL, NULL);
}

static int child_activations;
static HWND frame_client;

LONG FAR PASCAL _export MdiChildProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    if (msg == WM_MDIACTIVATE && wp)
        child_activations++;
    return DefMDIChildProc(hwnd, msg, wp, lp);
}

LONG FAR PASCAL _export FrameProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    return DefFrameProc(hwnd, frame_client, msg, wp, lp);
}

static void mdi_tests(HINSTANCE hInst)
{
    WNDCLASS wc;
    CLIENTCREATESTRUCT ccs;
    MDICREATESTRUCT mcs;
    HWND frame, c1, c2;
    HMENU menu, win;

    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = FrameProc;
    wc.hInstance = hInst;
    wc.hbrBackground = COLOR_APPWORKSPACE + 1;
    wc.lpszClassName = "W16Frame";
    RegisterClass(&wc);
    wc.lpfnWndProc = MdiChildProc;
    wc.hbrBackground = GetStockObject(WHITE_BRUSH);
    wc.lpszClassName = "W16Child";
    RegisterClass(&wc);
    menu = CreateMenu();
    win = CreatePopupMenu();
    AppendMenu(win, MF_STRING, 1, "&Tile");
    AppendMenu(menu, MF_POPUP, (UINT)win, "&Window");
    frame = CreateWindow("W16Frame", "W16TEST MDI", WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN, 30, 30, 400, 300, NULL, menu, hInst, NULL);
    ccs.hWindowMenu = win;
    ccs.idFirstChild = 1000;
    frame_client = CreateWindow("MDICLIENT", NULL, WS_CHILD | WS_CLIPCHILDREN | WS_VISIBLE, 0, 0, 300, 200, frame, (HMENU)1, hInst,
                                (LPSTR)&ccs);
    check(frame_client != 0, "CreateWindow MDICLIENT");
    ShowWindow(frame, SW_SHOW);
    memset(&mcs, 0, sizeof mcs);
    mcs.szClass = "W16Child";
    mcs.szTitle = "Child one";
    mcs.hOwner = hInst;
    mcs.x = mcs.y = mcs.cx = mcs.cy = CW_USEDEFAULT;
    c1 = (HWND)SendMessage(frame_client, WM_MDICREATE, 0, (LPARAM)(LPMDICREATESTRUCT)&mcs);
    mcs.szTitle = "Child two";
    c2 = (HWND)SendMessage(frame_client, WM_MDICREATE, 0, (LPARAM)(LPMDICREATESTRUCT)&mcs);
    check(c1 != 0 && c2 != 0 && GetParent(c1) == frame_client, "WM_MDICREATE");
    check(LOWORD(SendMessage(frame_client, WM_MDIGETACTIVE, 0, 0)) == (WORD)c2 && child_activations == 2, "WM_MDIGETACTIVE, WM_MDIACTIVATE");
    check(GetMenuItemCount(win) == 4 && GetMenuState(win, 1001, MF_BYCOMMAND) & MF_CHECKED, "the Window menu lists the children");
    SendMessage(frame_client, WM_MDIACTIVATE, (WPARAM)c1, 0);
    check(LOWORD(SendMessage(frame_client, WM_MDIGETACTIVE, 0, 0)) == (WORD)c1, "WM_MDIACTIVATE");
    SendMessage(frame_client, WM_MDIDESTROY, (WPARAM)c1, 0);
    check(!IsWindow(c1) && LOWORD(SendMessage(frame_client, WM_MDIGETACTIVE, 0, 0)) == (WORD)c2 && GetMenuItemCount(win) == 3, "WM_MDIDESTROY");
    DestroyWindow(frame);
}

static void misc_tests(void)
{
    CATCHBUF cb;
    static volatile int stage;
    int r;
    char buf[8];
    static char src[] = "hello world";
    OFSTRUCT of;
    HFILE hf;
    int fd;
    /* "ABCD" and a 4-byte copy of it: COMPRESS's SZDD format. */
    static BYTE szdd[] = { 'S', 'Z', 'D', 'D', 0x88, 0xf0, 0x27, 0x33, 'A', 'X', 8, 0, 0, 0, 0x0f, 'A', 'B', 'C', 'D', 0xf0, 0xf1 };

    stage = 0;
    r = Catch(cb);
    if (r == 0) {
        stage = 1;
        Throw(cb, 5);
        stage = 2;
    }
    check(r == 5 && stage == 1, "Catch/Throw");
    lstrcpyn(buf, src, sizeof buf);
    check(strcmp(buf, "hello w") == 0, "lstrcpyn");
    hmemcpy(buf, src + 6, 6);
    check(memcmp(buf, "world", 6) == 0, "hmemcpy");
    check(IsBadReadPtr(NULL, 1) && !IsBadReadPtr(src, sizeof src) && !IsBadWritePtr(buf, sizeof buf), "IsBadReadPtr/IsBadWritePtr");
    check(GetDriveType(2) == DRIVE_FIXED && GetDriveType(25) == 0, "GetDriveType");

    fd = open("C:\\W16LZ.TM_", O_CREAT | O_TRUNC | O_RDWR | O_BINARY, 0666);
    write(fd, szdd, sizeof szdd);
    close(fd);
    hf = LZOpenFile("C:\\W16LZ.TM_", &of, OF_READ);
    memset(buf, 0, sizeof buf);
    check(hf >= 0 && LZRead(hf, buf, 8) == 8 && memcmp(buf, "ABCDABCD", 8) == 0, "LZOpenFile/LZRead expand SZDD");
    LZClose(hf);
    unlink("C:\\W16LZ.TM_");
}

static void version_tests(void)
{
    char path[128];
    DWORD h, n;
    static char info[512];
    LPSTR v;
    UINT len;

    GetModuleFileName(hInstance, path, sizeof path);
    n = GetFileVersionInfoSize(path, &h);
    check(n > 0 && n <= sizeof info && GetFileVersionInfo(path, h, sizeof info, info), "GetFileVersionInfoSize/GetFileVersionInfo");
    check(VerQueryValue(info, "\\StringFileInfo\\040904E4\\FileVersion", (void FAR * FAR *)&v, &len) && lstrcmp(v, "1.0") == 0,
          "VerQueryValue");
}

/* The display's colours follow PM's: true colour, or 256 colours with a
 * palette (VIDEO=WxHx256). Every check passes in both. */
static int pal_changed, pal_query;
static HWND pal_from;

LONG FAR PASCAL _export PalWndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    switch (msg) {
    case WM_PALETTECHANGED:
        pal_changed++;
        pal_from = (HWND)wp;
        return 0;
    case WM_QUERYNEWPALETTE:
        pal_query++;
        return 0;
    }
    return DefWindowProc(hwnd, msg, wp, lp);
}

static void palette_tests(HINSTANCE hInst)
{
    WNDCLASS wc;
    HWND w;
    HDC dc, mem;
    HPALETTE hpal, old;
    HBITMAP bm, oldbm;
    BITMAP info;
    PALETTEENTRY sys[256], back[2];
    struct { WORD ver, n; PALETTEENTRY e[2]; } lp;
    static BYTE bits8[4] = { 0, 255, 0, 0 };
    static BYTE bits24[12] = { 0, 0, 0, 255, 255, 255 };
    BYTE got[12];
    MSG m;
    int bits, pal, n, i, found;
    COLORREF odd = RGB(0x12, 0x34, 0x56);

    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = PalWndProc;
    wc.hInstance = hInst;
    wc.hbrBackground = GetStockObject(WHITE_BRUSH);
    wc.lpszClassName = "W16Pal";
    RegisterClass(&wc);
    w = CreateWindow("W16Pal", "W16TEST palette", WS_OVERLAPPEDWINDOW, 40, 40, 160, 100, NULL, NULL, hInst, NULL);
    ShowWindow(w, SW_SHOWNORMAL);
    UpdateWindow(w);
    dc = GetDC(w);
    bits = GetDeviceCaps(dc, BITSPIXEL);
    pal = (GetDeviceCaps(dc, RASTERCAPS) & RC_PALETTE) != 0;
    wsprintf(line, "GetDeviceCaps: %d bits per pixel, RC_PALETTE %d", bits, pal);
    check((bits == 24 && !pal) || (bits == 8 && pal), line);
    if (pal) {
        check(GetDeviceCaps(dc, PLANES) == 1 && GetDeviceCaps(dc, NUMCOLORS) == 20 && GetDeviceCaps(dc, SIZEPALETTE) == 256
              && GetDeviceCaps(dc, NUMRESERVED) == 20 && GetDeviceCaps(dc, COLORRES) == 18,
              "256 colours: PLANES 1, NUMCOLORS 20, SIZEPALETTE 256, NUMRESERVED 20, COLORRES 18");
        check(pal_query > 0, "256 colours: WM_QUERYNEWPALETTE as the window was activated");
    } else {
        check(GetDeviceCaps(dc, NUMCOLORS) == -1 && GetDeviceCaps(dc, SIZEPALETTE) == 0, "true colour: NUMCOLORS -1, SIZEPALETTE 0");
    }

    /* The system palette: black first, white last. */
    n = GetSystemPaletteEntries(dc, 0, 256, sys);
    check(n == 256 && sys[0].peRed == 0 && sys[0].peGreen == 0 && sys[0].peBlue == 0 && sys[255].peRed == 255
          && sys[255].peGreen == 255 && sys[255].peBlue == 255, "GetSystemPaletteEntries: black first, white last");

    /* A DDB compatible with the display is in its format: 8-bit indexes
       into the system palette at 256 colours. */
    bm = CreateCompatibleBitmap(dc, 4, 1);
    GetObject(bm, sizeof info, &info);
    wsprintf(line, "CreateCompatibleBitmap of the display: %d bits per pixel, %d bytes a row", info.bmBitsPixel, info.bmWidthBytes);
    check(info.bmBitsPixel == bits && info.bmPlanes == 1 && info.bmWidthBytes == (bits == 8 ? 4 : 12), line);
    n = bits == 8 ? 4 : 12;
    SetBitmapBits(bm, n, bits == 8 ? (void FAR *)bits8 : (void FAR *)bits24);
    mem = CreateCompatibleDC(dc);
    oldbm = SelectObject(mem, bm);
    check(GetPixel(mem, 0, 0) == RGB(0, 0, 0) && GetPixel(mem, 1, 0) == RGB(255, 255, 255), "the DDB's bits (black, white)");
    memset(got, 0x55, sizeof got);
    check(GetBitmapBits(bm, n, got) == n && memcmp(got, bits == 8 ? bits8 : bits24, n) == 0, "GetBitmapBits of the display's format");
    SelectObject(mem, oldbm);
    DeleteDC(mem);
    DeleteObject(bm);

    /* A logical palette keeps its entries' flags. */
    lp.ver = 0x300;
    lp.n = 2;
    lp.e[0].peRed = 0x12; lp.e[0].peGreen = 0x34; lp.e[0].peBlue = 0x56; lp.e[0].peFlags = 0;
    lp.e[1].peRed = 0xfe; lp.e[1].peGreen = 0x01; lp.e[1].peBlue = 0x80; lp.e[1].peFlags = PC_RESERVED;
    hpal = CreatePalette((LOGPALETTE FAR *)&lp);
    check(GetPaletteEntries(hpal, 0, 2, back) == 2 && back[1].peRed == 0xfe && back[1].peFlags == PC_RESERVED,
          "GetPaletteEntries (with the flags)");

    /* Realized in the foreground: at 256 colours the system palette gets
       the colours (and the windows hear of it), in true colour there is
       nothing to realize. */
    old = SelectPalette(dc, hpal, FALSE);
    pal_changed = 0;
    n = RealizePalette(dc);
    while (PeekMessage(&m, 0, 0, 0, PM_REMOVE))
        DispatchMessage(&m);
    GetSystemPaletteEntries(dc, 0, 256, sys);
    for (i = 0, found = 0; i < 256; i++)
        if (sys[i].peRed == 0x12 && sys[i].peGreen == 0x34 && sys[i].peBlue == 0x56)
            found = 1;
    wsprintf(line, "RealizePalette: %d entries, in the system palette %d, WM_PALETTECHANGED %d (from %04x)", n, found, pal_changed, pal_from);
    check(pal ? n == 2 && found && pal_changed > 0 && pal_from == w : n == 0, line);
    check(GetNearestColor(dc, odd) == odd, "GetNearestColor of a realized colour");
    SetPixel(dc, 2, 2, PALETTEINDEX(0));
    check(GetPixel(dc, 2, 2) == odd, "PALETTEINDEX(0) draws the palette's colour");
    SelectPalette(dc, old, FALSE);
    RealizePalette(dc);
    check(DeleteObject(hpal), "DeleteObject of a palette");
    GetSystemPaletteEntries(dc, 0, 256, sys);
    for (i = 0, found = 0; i < 256; i++)
        if (sys[i].peRed == 0x12 && sys[i].peGreen == 0x34 && sys[i].peBlue == 0x56)
            found = 1;
    check(!found, "deleting the realized palette gives the system palette back");
    ReleaseDC(w, dc);
    DestroyWindow(w);
}

static void real_app_tests(HINSTANCE hInst)
{
    HWND hwnd = CreateWindow("Stage2", "W16TEST real programs", WS_OVERLAPPEDWINDOW, 20, 20, 200, 100, NULL, NULL, hInst, NULL);

    bitmap_tests();
    mapping_tests();
    region_tests();
    atom_tests(hwnd);
    profile_tests();
    mdi_tests(hInst);
    misc_tests();
    version_tests();
    DestroyWindow(hwnd);
}

/* What Delphi 1.0's IDE needed (docs/WIN16.md "Delphi 1.0"). */
static int sizes;
static LPARAM last_size;

LONG FAR PASCAL _export SizeWndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    if (msg == WM_SIZE) {
        sizes++;
        last_size = lp;
    }
    return DefWindowProc(hwnd, msg, wp, lp);
}

static WNDPROC combo_proc;

/* A superclass of COMBOBOX, as Delphi's VCL makes TComboBox: its own
 * procedure, the combo box's behind it. */
LONG FAR PASCAL _export SuperComboProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    return CallWindowProc((FARPROC)combo_proc, hwnd, msg, wp, lp);
}

static void delphi_tests(HINSTANCE hInst)
{
    WNDCLASS wc;
    HWND top, child, list, owner;
    HBITMAP bm;
    BITMAP b;
    HDC mem;
    RECT rc, crc;
    int i, n;

    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = SizeWndProc;
    wc.hInstance = hInst;
    wc.hbrBackground = GetStockObject(WHITE_BRUSH);
    wc.lpszClassName = "W16Size";
    RegisterClass(&wc);

    /* An overlapped window gets WM_SIZE when first shown, even if it was
     * sized while hidden (Delphi's forms lay out their children then). */
    sizes = 0;
    top = CreateWindow("W16Size", "sizes", WS_OVERLAPPEDWINDOW, 10, 10, 200, 150, NULL, NULL, hInst, NULL);
    check(sizes == 0, "an overlapped window created hidden: no WM_SIZE yet");
    SetWindowPos(top, 0, 10, 10, 220, 160, SWP_NOZORDER | SWP_NOACTIVATE);
    n = sizes;
    check(n == 1, "SetWindowPos of a hidden window: WM_SIZE");
    SetWindowPos(top, 0, 20, 20, 220, 160, SWP_NOZORDER | SWP_NOACTIVATE);
    check(sizes == n, "SetWindowPos to the same size: no WM_SIZE");
    ShowWindow(top, SW_SHOWNORMAL);
    GetClientRect(top, &rc);
    check(sizes == n + 1 && last_size == MAKELONG(rc.right, rc.bottom), "the first ShowWindow sends WM_SIZE");
    n = sizes;
    ShowWindow(top, SW_HIDE);
    ShowWindow(top, SW_SHOWNORMAL);
    check(sizes == n, "a second ShowWindow: no WM_SIZE");

    /* A child window gets WM_SIZE from CreateWindow, visible or not. */
    sizes = 0;
    child = CreateWindow("W16Size", "", WS_CHILD, 0, 0, 30, 20, top, (HMENU)5, hInst, NULL);
    check(sizes == 1 && last_size == MAKELONG(30, 20), "a hidden child window: WM_SIZE from CreateWindow");
    n = sizes;
    ShowWindow(child, SW_SHOW);
    check(sizes == n, "showing it: no second WM_SIZE");

    /* The system bitmaps: the combo box button is drawn. */
    bm = LoadBitmap(NULL, MAKEINTRESOURCE(OBM_COMBO));
    check(bm != 0 && GetObject(bm, sizeof b, &b) == sizeof b && b.bmWidth >= 13 && b.bmHeight >= 13, "LoadBitmap(NULL, OBM_COMBO)");
    if (bm) {
        COLORREF c0, cc;
        mem = CreateCompatibleDC(NULL);
        SelectObject(mem, bm);
        c0 = GetPixel(mem, 0, 0);
        cc = GetPixel(mem, b.bmWidth / 2, b.bmHeight / 2 - 1);
        n = 0;
        for (i = 1; i < b.bmHeight - 1; i++)
            if (GetPixel(mem, b.bmWidth / 2, i) == RGB(0, 0, 0))
                n++;
        check(n >= 2, "OBM_COMBO has its arrow");
        DeleteDC(mem);
        DeleteObject(bm);
        (void)c0; (void)cc;
    }
    bm = LoadBitmap(NULL, MAKEINTRESOURCE(OBM_DNARROW));
    check(bm != 0 && GetObject(bm, sizeof b, &b) == sizeof b && b.bmWidth == GetSystemMetrics(SM_CXVSCROLL), "LoadBitmap(NULL, OBM_DNARROW): a scroll bar wide");
    if (bm)
        DeleteObject(bm);

    /* A drop-down list: a child made a child of the desktop takes no
     * focus and is placed on the screen. */
    owner = CreateWindow("EDIT", "", WS_CHILD | WS_VISIBLE | WS_BORDER, 5, 5, 100, 20, top, (HMENU)6, hInst, NULL);
    SetFocus(owner);
    list = CreateWindow("LISTBOX", "", WS_CHILD | WS_BORDER | WS_VSCROLL | WS_HSCROLL, 0, 0, 100, 80, owner, (HMENU)7, hInst, NULL);
    check(SetParent(list, NULL) == owner, "SetParent(list, NULL): the old parent");
    SetWindowPos(list, HWND_TOP, 300, 200, 100, 80, SWP_NOACTIVATE | SWP_SHOWWINDOW);
    GetWindowRect(list, &rc);
    sprintf(line, "a child of the desktop is placed on the screen: (300,200)-(400,280), is (%d,%d)-(%d,%d)", rc.left, rc.top, rc.right, rc.bottom);
    check(rc.left == 300 && rc.top == 200 && rc.right == 400 && rc.bottom == 280, line);
    check(SetFocus(list) == 0 && GetFocus() == owner, "SetFocus on a child of the desktop fails");
    /* Its scroll bars show only when there is something to scroll. */
    SendMessage(list, LB_ADDSTRING, 0, (LPARAM)(LPSTR)"one");
    SendMessage(list, LB_ADDSTRING, 0, (LPARAM)(LPSTR)"two");
    GetClientRect(list, &crc);
    check(crc.right == 98 && crc.bottom == 78, "a list box with two items has no scroll bars");
    for (i = 0; i < 20; i++)
        SendMessage(list, LB_ADDSTRING, 0, (LPARAM)(LPSTR)"more");
    GetClientRect(list, &crc);
    check(crc.right < 98 && crc.bottom == 78, "and a vertical one with 22 items");
    SendMessage(list, LB_SETHORIZONTALEXTENT, 400, 0);
    GetClientRect(list, &crc);
    check(crc.bottom < 78 && SendMessage(list, LB_GETHORIZONTALEXTENT, 0, 0) == 400, "LB_SETHORIZONTALEXTENT wider than the list: a horizontal one");
    DestroyWindow(list);

    /* PeekMessage's message range: only that range is looked at (and
     * taken off the queue). */
    {
        MSG m1;
        PostMessage(top, WM_USER + 5, 1, 0);
        PostMessage(top, WM_USER + 6, 2, 0);
        check(PeekMessage(&m1, NULL, WM_USER + 6, WM_USER + 6, PM_REMOVE) && m1.message == WM_USER + 6 && m1.wParam == 2, "PeekMessage with a range takes that message");
        check(!PeekMessage(&m1, NULL, WM_USER + 6, WM_USER + 6, PM_NOREMOVE), "and none is left of the range");
        check(PeekMessage(&m1, NULL, 0, 0, PM_REMOVE) && m1.message == WM_USER + 5, "the message before it is still there");
    }

    /* A superclassed combo box made with the list's WS_VSCROLL and the
     * dropped height: no scroll bar of its own, the field's height. */
    {
        WNDCLASS cw;
        HWND cb;
        RECT wr;
        GetClassInfo(NULL, "COMBOBOX", &cw);
        combo_proc = cw.lpfnWndProc;
        cw.lpfnWndProc = SuperComboProc;
        cw.hInstance = hInst;
        cw.lpszClassName = "W16SuperCombo";
        check(RegisterClass(&cw) != 0, "RegisterClass of a COMBOBOX superclass");
        check(!RegisterClass(&cw), "the module's second class of the name fails");
        cb = CreateWindow("W16SuperCombo", "", WS_CHILD | WS_VISIBLE | WS_VSCROLL | CBS_DROPDOWN | CBS_HASSTRINGS, 5, 40, 120, 150, top, (HMENU)8, hInst, NULL);
        GetWindowRect(cb, &wr);
        GetClientRect(cb, &crc);
        sprintf(line, "a superclassed drop-down combo box is its field's height, no scroll bar (%dx%d, client %d)", wr.right - wr.left, wr.bottom - wr.top, crc.right);
        check(cb && wr.bottom - wr.top < 40 && crc.right == wr.right - wr.left, line);
        SetWindowPos(cb, 0, 0, 0, 120, 200, SWP_NOMOVE | SWP_NOZORDER);
        GetWindowRect(cb, &rc);
        check(rc.bottom - rc.top == wr.bottom - wr.top, "SetWindowPos's height is the dropped list's");
        SendMessage(cb, CB_ADDSTRING, 0, (LPARAM)(LPSTR)"x");
        check(SendMessage(cb, CB_GETCOUNT, 0, 0) == 1, "it is a combo box");
        DestroyWindow(cb);
    }
    DestroyWindow(top);
}

/* TOOLHELP (stage 4): the databases, and notifications from another
 * task (a process of its own here). */
static HTASK nfy_task, nfy_exit_task;
static int nfy_starts, nfy_exits;

BOOL FAR PASCAL _export TestNotify(WORD id, DWORD data)
{
    if (id == NFY_STARTTASK) {
        nfy_starts++;
        nfy_task = GetCurrentTask();
    } else if (id == NFY_EXITTASK) {
        nfy_exits++;
        nfy_exit_task = GetCurrentTask();
    }
    (void)data;
    return FALSE;
}

static int enum_count;

BOOL FAR PASCAL _export CountWnd(HWND hwnd, LPARAM lp)
{
    (void)hwnd;
    (void)lp;
    enum_count++;
    return TRUE;
}

static void toolhelp_tests(HINSTANCE hInst)
{
    TASKENTRY te;
    MODULEENTRY me;
    GLOBALENTRY ge;
    STACKTRACEENTRY ste;
    MEMMANINFO mmi;
    HTASK me_task = GetCurrentTask();
    BYTE buf[4];
    BOOL found = FALSE;
    MSG msg;
    DWORD t0;
    int i;

    te.dwSize = sizeof te;
    for (i = TaskFirst(&te); i; i = TaskNext(&te))
        if (te.hTask == me_task)
            found = TRUE;
    check(found, "TaskFirst/TaskNext find this task");
    te.dwSize = sizeof te;
    check(TaskFindHandle(&te, me_task) && te.hInst == hInst && lstrcmp(te.szModule, "W16TEST") == 0, "TaskFindHandle: instance and module name");
    check(*(WORD FAR *)MAKELP(me_task, 0xfa) == 0x4454 && *(WORD FAR *)MAKELP(me_task, 0x1c) == (WORD)hInst, "the task database: \"TD\", hInstance at 1Ch");
    me.dwSize = sizeof me;
    check(ModuleFindHandle(&me, te.hModule) && lstrcmp(me.szModule, "W16TEST") == 0, "ModuleFindHandle");
    me.dwSize = sizeof me;
    check(ModuleFindName(&me, "USER") && me.hModule == GetModuleHandle("USER"), "ModuleFindName(\"USER\")");
    ge.dwSize = sizeof ge;
    check(GlobalEntryHandle(&ge, (HGLOBAL)hInst) && ge.wType == GT_DGROUP, "GlobalEntryHandle: DGROUP");
    check(GlobalHandleToSel((HGLOBAL)hInst) == (WORD)hInst, "GlobalHandleToSel");
    ste.dwSize = sizeof ste;
    i = StackTraceFirst(&ste, NULL);
    sprintf(line, "StackTraceFirst: this task's stack (%d, SS %04X, CS:IP %04X:%04X)", i, ste.wSS, ste.wCS, ste.wIP);
    check(i && ste.wSS == SELECTOROF((void FAR *)&ste) && ste.wCS == SELECTOROF((void (FAR *)(HINSTANCE))toolhelp_tests), line);
    mmi.dwSize = sizeof mmi;
    check(MemManInfo(&mmi) && mmi.wPageSize == 4096 && mmi.dwTotalPages > 0, "MemManInfo");
    check(MemoryRead((WORD)me_task, 0xfa, buf, 2) == 2 && buf[0] == 'T' && buf[1] == 'D', "MemoryRead");

    /* Another task: its start and its end, through NotifyRegister. */
    check(NotifyRegister(NULL, (LPFNNOTIFYCALLBACK)TestNotify, NF_NORMAL), "NotifyRegister");
    check(WinExec("RS2\\HELLO16.EXE", SW_SHOWMINNOACTIVE) > 32, "WinExec(HELLO16) returns its instance");
    check(nfy_starts == 1 && nfy_task != 0 && nfy_task != me_task, "NFY_STARTTASK before WinExec returns, GetCurrentTask its task");
    te.dwSize = sizeof te;
    check(nfy_task && TaskFindHandle(&te, nfy_task) && lstrcmp(te.szModule, "HELLO16") == 0, "TaskFindHandle on the other task");
    check(IsTask(nfy_task) && GetCurrentTask() == me_task, "IsTask; GetCurrentTask is this task again");
    {
        /* EnumTaskWindows: this task's windows, none of them another's. */
        HWND tw = CreateWindow("W16Test", "enum", WS_OVERLAPPED, 0, 0, 50, 50, NULL, NULL, hInst, NULL);
        int own;
        enum_count = 0;
        EnumTaskWindows(me_task, (WNDENUMPROC)CountWnd, 0);
        own = enum_count;
        enum_count = 0;
        EnumTaskWindows(nfy_task, (WNDENUMPROC)CountWnd, 0);
        check(tw && own >= 1 && enum_count == 0, "EnumTaskWindows: this task's windows, not for another task");
        DestroyWindow(tw);
    }
    check(GetSystemDebugState() == 0, "GetSystemDebugState: no menu, no system-modal window");
    TerminateApp(nfy_task, NO_UAE_BOX);
    t0 = GetTickCount();
    while (nfy_exits == 0 && GetTickCount() - t0 < 5000)
        if (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE))
            DispatchMessage(&msg);
    check(nfy_exits == 1 && nfy_exit_task == nfy_task, "NFY_EXITTASK for it");
    NotifyUnRegister(NULL);
}

/* INT 2Fh AX=1000h: SHARE's installed state (FFh: loaded). */
BYTE share_state(void);
#pragma aux share_state = "mov ax, 1000h" "int 2Fh" value [al] modify [ax];

/* INT 21h AH=30h with EBX's upper half set (386 code keeps 32-bit
 * values in registers across DOS calls): EBX's upper half after it. */
WORD int21_keeps_ebx(void);
#pragma aux int21_keeps_ebx = \
    0x66 0xbb 0x78 0x56 0x34 0x12 /* mov ebx, 12345678h */ \
    0xb4 0x30                     /* mov ah, 30h */ \
    0xcd 0x21                     /* int 21h */ \
    0x66 0x8b 0xc3                /* mov eax, ebx */ \
    0x66 0xc1 0xe8 0x10           /* shr eax, 16 */ \
    value [ax] modify [ax bx cx dx];

/* The pixel at (x, y) of a window, once it has painted. */
/* The colour a pel of `c` shows (the nearest palette colour on a
 * 256-colour display). */
static COLORREF shown(COLORREF c)
{
    HDC hdc = GetDC(NULL);
    c = GetNearestColor(hdc, c);
    ReleaseDC(NULL, hdc);
    return c;
}

static COLORREF pixel_at(HWND w, int x, int y)
{
    HDC hdc;
    COLORREF c;

    UpdateWindow(w);
    hdc = GetDC(w);
    c = GetPixel(hdc, x, y);
    ReleaseDC(w, hdc);
    return c;
}

/* What Delphi 1.0's installer (Borland's INSTALL.EXE, INSTRUN) needs. */
static void installer_tests(HINSTANCE hInst)
{
    BYTE FAR *bda = (BYTE FAR *)MAKELP(0x40, 0x40);
    BYTE old;
    HRSRC r;
    HGLOBAL g;
    LPSTR p;
    HWND parent, b1, frame, b2, box, b3;
    COLORREF c;
    char buf[80];

    /* The BIOS data area is writable: setup programs keep the floppy
     * motor running through 40:40. */
    old = *bda;
    *bda = 0xff;
    check(*bda == 0xff, "the BIOS data area (40:40) is writable");
    *bda = old;

    /* Named types through RT_NAMETABLE. */
    r = FindResource(hInst, "INSTALL", "SCR");
    check(r != 0, "FindResource: a named type and name from the RT_NAMETABLE");
    g = r ? LoadResource(hInst, r) : 0;
    p = g ? LockResource(g) : NULL;
    check(p != NULL && lstrcmp(p, "script") == 0, "LoadResource of a resource named in the RT_NAMETABLE");
    if (g) {
        FreeResource(g);
    }
    check(FindResource(hInst, "NOTTHERE", "SCR") == 0, "FindResource: a name not in the name table");

    /* Child windows in Windows' Z order (the first created on top) and
     * frames that show what is inside them: a button created before a
     * frame static that covers it, a frame static and a group box
     * created before the buttons inside them. */
    parent = CreateWindow("Stage2", "zorder", WS_OVERLAPPED | WS_CAPTION | WS_VISIBLE, 40, 40, 360, 140, NULL, NULL, hInst, NULL);
    b1 = CreateWindow("BUTTON", "One", WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON, 20, 20, 60, 30, parent, (HMENU)1, hInst, NULL);
    frame = CreateWindow("STATIC", "", WS_CHILD | WS_VISIBLE | SS_GRAYFRAME, 10, 10, 100, 80, parent, (HMENU)2, hInst, NULL);
    check(GetWindow(parent, GW_CHILD) == b1 && GetWindow(b1, GW_HWNDNEXT) == frame, "children in creation order, the first on top (GW_CHILD, GW_HWNDNEXT)");
    c = pixel_at(parent, 25, 35);
    wsprintf(buf, "a button under a later frame static shows, is %06lX", c);
    check(c == shown(GetSysColor(COLOR_BTNFACE)), buf);
    c = pixel_at(parent, 95, 75);
    wsprintf(buf, "a frame static's inside is its parent's background, is %06lX", c);
    check(c == RGB(255, 255, 255), buf);
    DestroyWindow(frame);
    DestroyWindow(b1);
    frame = CreateWindow("STATIC", "", WS_CHILD | WS_VISIBLE | SS_BLACKFRAME, 10, 10, 100, 80, parent, (HMENU)2, hInst, NULL);
    b2 = CreateWindow("BUTTON", "Two", WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON, 20, 20, 60, 30, parent, (HMENU)3, hInst, NULL);
    box = CreateWindow("BUTTON", "Group", WS_CHILD | WS_VISIBLE | BS_GROUPBOX, 130, 5, 200, 90, parent, (HMENU)4, hInst, NULL);
    b3 = CreateWindow("BUTTON", "Three", WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON, 150, 30, 60, 30, parent, (HMENU)5, hInst, NULL);
    c = pixel_at(parent, 25, 35);
    wsprintf(buf, "a button inside an earlier frame static shows, is %06lX", c);
    check(c == shown(GetSysColor(COLOR_BTNFACE)), buf);
    c = pixel_at(parent, 155, 45);
    wsprintf(buf, "a button inside an earlier group box shows, is %06lX", c);
    check(c == shown(GetSysColor(COLOR_BTNFACE)), buf);
    c = pixel_at(parent, 300, 70);
    wsprintf(buf, "a group box's inside is its parent's background, is %06lX", c);
    check(c == RGB(255, 255, 255), buf);
    ShowWindow(b2, SW_HIDE);
    ShowWindow(b2, SW_SHOW);
    check(GetWindow(parent, GW_CHILD) == frame, "ShowWindow keeps a child's place in the Z order");
    DestroyWindow(parent);

    /* EM_SETHANDLE: an edit control's text from a local block of its
     * instance's segment (the installer's README window). */
    {
        HWND e = CreateWindow("EDIT", "", WS_POPUP | ES_MULTILINE, 0, 0, 200, 100, NULL, NULL, hInst, NULL);
        HLOCAL hl = LocalAlloc(LMEM_MOVEABLE, 32);
        lstrcpy(LocalLock(hl), "Read me\r\nfirst");
        LocalUnlock(hl);
        SendMessage(e, EM_SETHANDLE, (WPARAM)hl, 0);
        GetWindowText(e, buf, sizeof buf);
        check(lstrcmp(buf, "Read me\r\nfirst") == 0 && SendMessage(e, EM_GETLINECOUNT, 0, 0) == 2, "EM_SETHANDLE: the text of a local block");
        DestroyWindow(e);
    }

    /* The registration database (SHELL), and REGEDIT /s merging a .REG
     * file (the installer registers OLE 2 so). */
    {
        HKEY k, k2;
        LONG cb;
        HFILE f;
        static const char reg[] = "REGEDIT\r\nHKEY_CLASSES_ROOT\\W16Merged = merged value\r\nHKEY_CLASSES_ROOT\\W16Merged\\sub = deep\r\n";

        check(RegCreateKey(HKEY_CLASSES_ROOT, "W16Test\\Shell", &k) == ERROR_SUCCESS, "RegCreateKey (and its parent)");
        check(RegSetValue(k, "open", REG_SZ, "w16test.exe %1", 14) == ERROR_SUCCESS, "RegSetValue");
        cb = sizeof buf;
        check(RegQueryValue(HKEY_CLASSES_ROOT, "W16Test\\Shell\\open", buf, &cb) == ERROR_SUCCESS && lstrcmp(buf, "w16test.exe %1") == 0 && cb == 15,
              "RegQueryValue: the value and its size");
        check(RegOpenKey(HKEY_CLASSES_ROOT, "W16Test", &k2) == ERROR_SUCCESS && RegEnumKey(k2, 0, buf, sizeof buf) == ERROR_SUCCESS && lstrcmp(buf, "Shell") == 0 &&
              RegEnumKey(k2, 1, buf, sizeof buf) != ERROR_SUCCESS, "RegOpenKey, RegEnumKey");
        RegCloseKey(k);
        check(RegDeleteKey(k2, NULL) == ERROR_SUCCESS && RegOpenKey(HKEY_CLASSES_ROOT, "W16Test\\Shell", &k) != ERROR_SUCCESS, "RegDeleteKey (and the keys below)");
        RegCloseKey(k2);
        check(RegOpenKey(HKEY_CLASSES_ROOT, "W16NoSuchKey", &k) == ERROR_BADKEY, "RegOpenKey of a missing key: ERROR_BADKEY");
        f = _lcreat("C:\\W16TEST.REG", 0);
        _lwrite(f, reg, sizeof reg - 1);
        _lclose(f);
        check(WinExec("REGEDIT /s C:\\W16TEST.REG", SW_HIDE) > 32, "WinExec REGEDIT /s (no REGEDIT.EXE)");
        cb = sizeof buf;
        check(RegQueryValue(HKEY_CLASSES_ROOT, "W16Merged\\sub", buf, &cb) == ERROR_SUCCESS && lstrcmp(buf, "deep") == 0, "REGEDIT merged the .REG file");
        RegDeleteKey(HKEY_CLASSES_ROOT, "W16Merged");
    }

    /* WinExec of a file that is no program: Windows' error 11 (and WIN16
     * says why on standard error and the serial port, as setup programs
     * that only start the next one say nothing). */
    {
        static char junk[] = "this is not a program";
        OFSTRUCT bad;
        HFILE f = _lcreat("C:\\W16BAD.EXE", 0);
        _lwrite(f, junk, sizeof junk);
        _lclose(f);
        check(WinExec("C:\\W16BAD.EXE", SW_SHOWNORMAL) == 11, "WinExec of a file that is not a program: 11");
        OpenFile("C:\\W16BAD.EXE", &bad, OF_DELETE);
    }

    /* What Windows Setup writes: WIN.INI's [intl], SYSTEM.INI's code
     * page; SHARE is loaded (INT 2Fh AX=1000h). */
    check(GetProfileInt("intl", "iCountry", 0) != 0, "WIN.INI [intl] iCountry from the country settings");
    GetProfileString("intl", "sLanguage", "", buf, sizeof buf);
    check(lstrlen(buf) == 3, "WIN.INI [intl] sLanguage");
    check(GetPrivateProfileInt("boot.description", "codepage", 0, "SYSTEM.INI") != 0, "SYSTEM.INI [boot.description] codepage");
    check(share_state() == 0xff, "INT 2Fh AX=1000h: SHARE is loaded");
    check(int21_keeps_ebx() == 0x1234, "INT 21h keeps the upper half of a 32-bit register");

    /* A huge block's selectors reach its end, as on Windows (Borland's
     * Quantum decompressor addresses its 256 KB window with 32-bit
     * offsets from the first). */
    {
        HGLOBAL g = GlobalAlloc(GMEM_MOVEABLE, 0x40000L);
        void FAR *p = GlobalLock(g);
        UINT sel = p ? SELECTOROF(p) : 0;
        check(sel && GetSelectorLimit(sel) == 0x3ffffL && GetSelectorLimit(sel + 8) == 0x2ffffL, "GlobalAlloc 256 KB: the first selector reaches the whole block");
        GlobalUnlock(g);
        GlobalFree(g);
    }

    /* Program Manager's DDE, as setup programs use it (WIN16's stand-in
     * when no Program Manager runs). */
    {
        DWORD inst = 0, res;
        HSZ pm;
        HCONV conv;
        HDDEDATA d;
        HSZ groups;
        static char text[200];
        static const char cmd[] = "[CreateGroup(\"W16 Test Group\")][ShowGroup(\"W16 Test Group\", 1)]";
        static const char del[] = "[DeleteGroup(\"W16 Test Group\")]";

        DdeInitialize(&inst, (PFNCALLBACK)MakeProcInstance((FARPROC)CliCallback, hInstance), APPCMD_CLIENTONLY, 0);
        pm = DdeCreateStringHandle(inst, "PROGMAN", CP_WINANSI);
        conv = DdeConnect(inst, pm, pm, NULL);
        check(conv != 0, "DdeConnect to PROGMAN");
        if (conv) {
            check(DdeClientTransaction((LPBYTE)cmd, sizeof cmd, conv, 0, 0, XTYP_EXECUTE, 5000, &res) && (res & DDE_FACK),
                  "PROGMAN: [CreateGroup][ShowGroup]");
            GetPrivateProfileString("Group Names", "C:\\WINDOWS\\W16TESTG.GRP", "", text, sizeof text, "PROGMAN.INI");
            check(lstrcmp(text, "W16 Test Group") == 0, "PROGMAN: the group in PROGMAN.INI");
            groups = DdeCreateStringHandle(inst, "Groups", CP_WINANSI);
            d = DdeClientTransaction(NULL, 0, conv, groups, CF_TEXT, XTYP_REQUEST, 5000, &res);
            text[0] = 0;
            if (d) {
                DdeGetData(d, text, sizeof text, 0);
                DdeFreeDataHandle(d);
            }
            check(strstr(text, "W16 Test Group\r\n") != NULL, "PROGMAN: XTYP_REQUEST Groups lists it");
            check(DdeClientTransaction((LPBYTE)del, sizeof del, conv, 0, 0, XTYP_EXECUTE, 5000, &res) && (res & DDE_FACK),
                  "PROGMAN: [DeleteGroup]");
            GetPrivateProfileString("Group Names", "C:\\WINDOWS\\W16TESTG.GRP", "", text, sizeof text, "PROGMAN.INI");
            check(text[0] == 0, "PROGMAN: the group gone from PROGMAN.INI");
            DdeDisconnect(conv);
        }
        DdeUninitialize(inst);
    }
}

/* Delphi's TNotebook (Environment Options): a page is made 0 high, the
 * controls of a page not shown before are made hidden, each shown with
 * SetWindowPos while the page is still hidden, the page is shown and
 * then sized to the notebook. The controls keep their places from the
 * page's top and are on the screen. */
static int drawn(HWND h)
{
    HDC hdc;
    RECT rc;
    int x, y, n = 0;

    UpdateWindow(GetParent(GetParent(h)));
    hdc = GetDC(h);
    GetClientRect(h, &rc);
    for (y = 0; y < rc.bottom; y += 2)
        for (x = 0; x < 16 && x < rc.right; x++)
            if (GetPixel(hdc, x, y) != RGB(255, 255, 255))
                n++;
    ReleaseDC(h, hdc);
    return n;
}

static void notebook_tests(HINSTANCE hInst)
{
    const UINT show = SWP_NOSIZE | SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW;
    HWND top, page, group, cb;
    MSG msg;
    RECT pr, cr;

    top = CreateWindow("W16Size", "notebook", WS_OVERLAPPEDWINDOW | WS_VISIBLE, 40, 40, 300, 220, NULL, NULL, hInst, NULL);
    page = CreateWindow("W16Size", "", WS_CHILD | WS_CLIPCHILDREN, 0, 0, 0, 0, top, NULL, hInst, NULL);
    group = CreateWindow("BUTTON", "Group", WS_CHILD | WS_CLIPSIBLINGS | BS_GROUPBOX, 10, 10, 220, 100, page, NULL, hInst, NULL);
    cb = CreateWindow("BUTTON", "Check", WS_CHILD | WS_CLIPSIBLINGS | BS_AUTOCHECKBOX, 10, 30, 150, 20, group, NULL, hInst, NULL);
    SetWindowPos(cb, 0, 0, 0, 0, 0, SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE);
    SetWindowPos(cb, 0, 0, 0, 0, 0, show);
    SetWindowPos(group, 0, 0, 0, 0, 0, show);
    SetWindowPos(page, 0, 0, 0, 0, 0, show);
    SetWindowPos(page, 0, 0, 0, 280, 170, SWP_NOZORDER | SWP_NOACTIVATE);
    while (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE))
        DispatchMessage(&msg);
    check(IsWindowVisible(cb) && IsWindowVisible(group), "a page shown after its controls: they are visible");
    GetWindowRect(page, &pr);
    GetWindowRect(cb, &cr);
    check(cr.top - pr.top == 40 && cr.left - pr.left == 20, "a page sized after its controls: they keep their places from its top");
    check(drawn(cb) > 0, "a page shown after its controls: they are drawn");
    DestroyWindow(top);
}

/* SetWindowPos with SWP_NOMOVE keeps the top-left corner (Delphi makes
 * a combo box as high as its dropped list this way, as the user clicks
 * it: the field must stay where it is, under the mouse). */
static void nomove_tests(HINSTANCE hInst)
{
    HWND top, cb, st;
    RECT pr, r;

    top = CreateWindow("W16Size", "nomove", WS_OVERLAPPEDWINDOW | WS_VISIBLE, 40, 40, 300, 220, NULL, NULL, hInst, NULL);
    cb = CreateWindow("COMBOBOX", "", WS_CHILD | WS_VISIBLE | CBS_DROPDOWNLIST, 10, 30, 150, 100, top, NULL, hInst, NULL);
    st = CreateWindow("STATIC", "x", WS_CHILD | WS_VISIBLE, 10, 80, 50, 20, top, NULL, hInst, NULL);
    SendMessage(cb, CB_ADDSTRING, 0, (LPARAM)(LPSTR)"one");
    GetWindowRect(cb, &r);
    ScreenToClient(top, (POINT FAR *)&r);
    SetWindowPos(cb, 0, 0, 0, 150, 120, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    GetWindowRect(cb, &pr);
    ScreenToClient(top, (POINT FAR *)&pr);
    check(pr.top == r.top && pr.left == r.left, "SetWindowPos SWP_NOMOVE of a combo box (its dropped height): the field stays");
    SetWindowPos(st, 0, 0, 0, 50, 60, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
    GetWindowRect(st, &r);
    r.bottom -= r.top;
    ScreenToClient(top, (POINT FAR *)&r);
    check(r.top == 80 && r.left == 10 && r.bottom == 60, "SetWindowPos SWP_NOMOVE: the top-left corner stays, the window grows down");
    DestroyWindow(top);
}

/* A program that loaded a DLL and ended (without freeing it) leaves no
 * trace: the DLL is not loaded here (Delphi refuses to start a second
 * time when GetModuleHandle finds COMPLIB.DCL), and loading it runs its
 * LibMain in this process. */
/* A task of this module is running (TaskFirst/TaskNext). */
static BOOL task_running(const char *module)
{
    TASKENTRY te;
    BOOL ok;

    te.dwSize = sizeof te;
    for (ok = TaskFirst(&te); ok; ok = TaskNext(&te))
        if (lstrcmp(te.szModule, module) == 0)
            return TRUE;
    return FALSE;
}

static void dll_tests(void)
{
    char buf[8];
    DWORD t0;
    MSG msg;
    HINSTANCE d;
    int (FAR PASCAL *f)(void);

    WritePrivateProfileString("run", "value", "", "W16DLL.INI");
    check(WinExec("C:\\RS2\\HELLO16.EXE DLL", SW_SHOWMINNOACTIVE) > 32, "WinExec(HELLO16 DLL): it loads W16DLL and ends");
    /* Its whole value: the file may be read as it is being written. */
    t0 = GetTickCount();
    buf[0] = 0;
    while (lstrcmp(buf, "42") != 0 && lstrcmp(buf, "0") != 0 && GetTickCount() - t0 < 30000) {
        if (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE))
            DispatchMessage(&msg);
        GetPrivateProfileString("run", "value", "", buf, sizeof buf, "W16DLL.INI");
    }
    check(lstrcmp(buf, "42") == 0, "the other program's W16DLL ran its LibMain");
    /* It ends right after writing: wait until it has (no time is long
     * enough on a busy machine). */
    t0 = GetTickCount();
    while (task_running("HELLO16") && GetTickCount() - t0 < 30000)
        if (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE))
            DispatchMessage(&msg);
    check(!task_running("HELLO16"), "HELLO16 ended (TaskFirst/TaskNext no longer list it)");
    check(GetModuleHandle("W16DLL") == 0, "a DLL of a program that ended is not loaded here (GetModuleHandle)");
    d = LoadLibrary("W16DLL.DLL");
    f = d > HINSTANCE_ERROR ? (int (FAR PASCAL *)(void))GetProcAddress(d, "W16DLLVALUE") : NULL;
    check(f && f() == 42, "LoadLibrary of it again: its LibMain runs here");
    check(GetModuleHandle("W16DLL") == d, "GetModuleHandle finds it now");
    if (d > HINSTANCE_ERROR)
        FreeLibrary(d);
}

/* The Delphi RAD Pack (Resource Workshop, the Visual Solutions Pack and
 * its installer's BWCC dialogs, Borland's VBX runtime). */
static int rad_paints, rad_child_paints, rad_activates, ord_seen, icon_seen;
static WORD ord_id;

LONG FAR PASCAL _export RadWndProc(HWND h, UINT msg, WPARAM wp, LPARAM lp)
{
    PAINTSTRUCT ps;

    switch (msg) {
    case WM_ACTIVATE:
        if (wp != WA_INACTIVE)
            rad_activates++;
        break;
    case WM_ERASEBKGND:
        /* As Borland's VBX runtime does. */
        if (GetProp(h, "inval"))
            InvalidateRect(h, NULL, FALSE);
        break;
    case WM_PAINT:
        BeginPaint(h, &ps);
        EndPaint(h, &ps);
        if (GetProp(h, "child"))
            rad_child_paints++;
        else
            rad_paints++;
        return 0;
    }
    return DefWindowProc(h, msg, wp, lp);
}

LONG FAR PASCAL _export OrdWndProc(HWND h, UINT msg, WPARAM wp, LPARAM lp)
{
    LPCSTR n;

    if (msg == WM_CREATE) {
        n = ((CREATESTRUCT FAR *)lp)->lpszName;
        if (n != NULL && (BYTE)n[0] == 0xff) {
            ord_seen = 1;
            ord_id = *(WORD FAR *)(n + 1);
        }
    }
    return DefWindowProc(h, msg, wp, lp);
}

BOOL FAR PASCAL _export RadDlgProc(HWND h, UINT msg, WPARAM wp, LPARAM lp)
{
    if (msg == WM_INITDIALOG) {
        icon_seen = SendDlgItemMessage(h, IDC_RADICON, WM_USER + 1, 0, 0) != 0;
        EndDialog(h, 1);
        return TRUE;
    }
    return FALSE;
}

static void rad_pump(DWORD ms)
{
    DWORD t0 = GetTickCount();
    MSG msg;

    do {
        while (PeekMessage(&msg, NULL, 0, 0, PM_REMOVE))
            DispatchMessage(&msg);
    } while (GetTickCount() - t0 < ms);
}

static COLORREF rad_pixel(HWND h, int x, int y)
{
    HDC dc = GetDC(h);
    COLORREF c = GetPixel(dc, x, y);

    ReleaseDC(h, dc);
    return c;
}

static void fill_red(HWND h, int cx, int cy)
{
    HDC dc = GetDC(h);
    HBRUSH b = CreateSolidBrush(RGB(255, 0, 0));
    RECT r;

    SetRect(&r, 0, 0, cx, cy);
    FillRect(dc, &r, b);
    DeleteObject(b);
    ReleaseDC(h, dc);
}

static HLOCAL alloc_in(WORD sel)
{
    HLOCAL h;

    _asm {
        push ds
        mov ax, sel
        mov ds, ax
    }
    h = LocalAlloc(LMEM_FIXED, 16);
    _asm {
        pop ds
    }
    return h;
}

static WORD bios_equipment(void)
{
    WORD r;

    _asm {
        int 11h
        mov r, ax
    }
    return r;
}

static WORD bios_video_mode(void)
{
    WORD r;

    _asm {
        mov ah, 0fh
        int 10h
        mov r, ax
    }
    return r;
}

static void radpack_tests(HINSTANCE hInst)
{
    WNDCLASS wc;
    HWND top, par, kid, a, b, w, st, gb, pb, ch;
    HMENU sm;
    RECT r, cr;
    HGLOBAL g;
    WORD sel;
    HLOCAL hl;
    int n;
    FARPROC dp;

    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = RadWndProc;
    wc.hInstance = hInst;
    wc.hCursor = LoadCursor(NULL, IDC_ARROW);
    wc.hbrBackground = GetStockObject(WHITE_BRUSH);
    wc.lpszClassName = "W16Rad";
    RegisterClass(&wc);
    wc.lpfnWndProc = OrdWndProc;
    wc.lpszClassName = "W16Ord";
    RegisterClass(&wc);

    /* A framed window hears of its activation. */
    rad_activates = 0;
    top = CreateWindow("W16Rad", "radpack", WS_OVERLAPPEDWINDOW, 40, 40, 360, 260, NULL, NULL, hInst, NULL);
    ShowWindow(top, SW_SHOWNORMAL);
    UpdateWindow(top);
    rad_pump(300);
    check(rad_activates > 0, "a window with a frame gets WM_ACTIVATE (BWCC dialogs give IsDialogMessage their dialog then)");

    /* WS_CLIPCHILDREN and WS_CLIPSIBLINGS as the program sets them. */
    par = CreateWindow("W16Rad", "", WS_CHILD | WS_VISIBLE, 0, 0, 200, 150, top, NULL, hInst, NULL);
    kid = CreateWindow("STATIC", "", WS_CHILD | WS_VISIBLE | SS_WHITERECT, 20, 20, 40, 40, par, NULL, hInst, NULL);
    rad_pump(200);
    fill_red(par, 100, 100);
    check(rad_pixel(kid, 5, 5) == RGB(255, 0, 0), "a window without WS_CLIPCHILDREN draws over its children (BWCC's dialogs draw their texts so)");
    SetWindowLong(par, GWL_STYLE, GetWindowLong(par, GWL_STYLE) | WS_CLIPCHILDREN);
    InvalidateRect(kid, NULL, TRUE);
    UpdateWindow(kid);
    fill_red(par, 100, 100);
    check(rad_pixel(kid, 5, 5) == RGB(255, 255, 255), "with WS_CLIPCHILDREN it does not");
    DestroyWindow(par);
    par = CreateWindow("W16Rad", "", WS_CHILD | WS_VISIBLE, 0, 0, 200, 150, top, NULL, hInst, NULL);
    a = CreateWindow("STATIC", "", WS_CHILD | WS_VISIBLE | SS_BLACKRECT, 10, 10, 60, 60, par, NULL, hInst, NULL);
    b = CreateWindow("W16Rad", "", WS_CHILD | WS_VISIBLE, 40, 40, 60, 60, par, NULL, hInst, NULL);
    rad_pump(200);
    fill_red(b, 60, 60);
    check(rad_pixel(par, 50, 50) == RGB(255, 0, 0), "a child without WS_CLIPSIBLINGS draws over the sibling above it (controls under BWCC's group shades)");
    SetWindowLong(b, GWL_STYLE, GetWindowLong(b, GWL_STYLE) | WS_CLIPSIBLINGS);
    InvalidateRect(a, NULL, TRUE);
    UpdateWindow(a);
    fill_red(b, 60, 60);
    check(rad_pixel(par, 50, 50) == RGB(0, 0, 0), "with WS_CLIPSIBLINGS it does not");
    DestroyWindow(par);

    /* A parent without WS_CLIPCHILDREN paints over its children: they
     * paint again after it, even when they were updated before it
     * (Resource Workshop's dialog editor). */
    par = CreateWindow("W16Rad", "", WS_CHILD | WS_VISIBLE, 0, 0, 200, 150, top, NULL, hInst, NULL);
    kid = CreateWindow("W16Rad", "", WS_CHILD | WS_VISIBLE, 20, 20, 40, 40, par, NULL, hInst, NULL);
    SetProp(kid, "child", (HANDLE)1);
    rad_pump(200);
    InvalidateRect(kid, NULL, TRUE);
    InvalidateRect(par, NULL, TRUE);
    UpdateWindow(kid);
    rad_child_paints = 0;
    UpdateWindow(par);
    rad_pump(200);
    check(rad_child_paints > 0, "children paint again after their non-clipping parent paints");
    RemoveProp(kid, "child");
    DestroyWindow(par);

    /* An invalidation while the window erases its background belongs to
     * that painting (Borland's VBX runtime; each painting asked for the
     * next one). */
    w = CreateWindow("W16Rad", "", WS_CHILD | WS_VISIBLE, 0, 0, 100, 100, top, NULL, hInst, NULL);
    SetProp(w, "inval", (HANDLE)1);
    rad_pump(200);
    rad_paints = 0;
    InvalidateRect(w, NULL, TRUE);
    UpdateWindow(w);
    rad_pump(300);
    check(rad_paints == 1 && !GetUpdateRect(w, NULL, FALSE), "InvalidateRect on WM_ERASEBKGND: one WM_PAINT, no more");
    RemoveProp(w, "inval");
    DestroyWindow(w);

    /* Child windows with a caption: AdjustWindowRect's caption is there. */
    ch = CreateWindow("W16Rad", "caption", WS_CHILD | WS_CAPTION | WS_VISIBLE, 10, 10, 200, 100, top, NULL, hInst, NULL);
    GetClientRect(ch, &cr);
    SetRect(&r, 0, 0, cr.right, cr.bottom);
    AdjustWindowRect(&r, WS_CHILD | WS_CAPTION, FALSE);
    check(r.right - r.left == 200 && r.bottom - r.top == 100, "a child window with WS_CAPTION: its caption and border are AdjustWindowRect's");
    DestroyWindow(ch);

    /* Hit testing: statics and group boxes let the mouse through. */
    st = CreateWindow("STATIC", "text", WS_CHILD | WS_VISIBLE, 10, 10, 60, 20, top, NULL, hInst, NULL);
    gb = CreateWindow("BUTTON", "group", WS_CHILD | WS_VISIBLE | BS_GROUPBOX, 10, 40, 100, 60, top, NULL, hInst, NULL);
    pb = CreateWindow("BUTTON", "push", WS_CHILD | WS_VISIBLE | BS_PUSHBUTTON, 120, 40, 60, 24, top, NULL, hInst, NULL);
    check(SendMessage(st, WM_NCHITTEST, 0, 0L) == HTTRANSPARENT && SendMessage(gb, WM_NCHITTEST, 0, 0L) == HTTRANSPARENT,
          "statics and group boxes answer WM_NCHITTEST with HTTRANSPARENT");
    check(SendMessage(pb, WM_NCHITTEST, 0, 0L) == HTCLIENT, "a push button answers HTCLIENT");
    DestroyWindow(st);
    DestroyWindow(gb);
    DestroyWindow(pb);

    /* SetCapture(NULL) lets the mouse go (Resource Workshop's palette). */
    SetCapture(top);
    check(GetCapture() == top, "SetCapture");
    check(SetCapture(NULL) == top && GetCapture() == NULL, "SetCapture(NULL) releases the mouse");

    /* The system menu, which programs add to (Resource Workshop). */
    sm = GetSystemMenu(top, FALSE);
    n = sm ? GetMenuItemCount(sm) : 0;
    check(sm != 0 && n > 0 && AppendMenu(sm, MF_STRING, 0x120, "Extra") && GetMenuItemCount(sm) == n + 1, "GetSystemMenu, AppendMenu to it");
    DestroyWindow(top);

    /* Ordinal texts in a dialog template. */
    dp = MakeProcInstance((FARPROC)RadDlgProc, hInst);
    ord_seen = icon_seen = 0;
    DialogBox(hInst, "RadDlg", NULL, dp);
    FreeProcInstance(dp);
    check(ord_seen && ord_id == IDI_TEST, "a control's ordinal text reaches it as 0xFF and the number (CREATESTRUCT)");
    check(icon_seen, "an icon static shows its template's icon (STM_GETICON)");

    /* LocalInit(seg, 0, size): the heap at the segment's end. */
    g = GlobalAlloc(GMEM_FIXED | GMEM_ZEROINIT, 0x1000);
    sel = HIWORD((DWORD)GlobalLock(g));
    check(LocalInit(sel, 0, 0x200) && (hl = alloc_in(sel)) != 0 && (WORD)hl >= 0x1000 - 0x200 - 4,
          "LocalInit(seg, 0, size): the heap is at the end of the segment (WORKLIB1's static data stays)");
    GlobalUnlock(g);
    GlobalFree(g);

    check((bios_equipment() & 2) != 0, "INT 11h: the equipment list has the coprocessor");
    check(LOBYTE(bios_video_mode()) == 3, "INT 10h AH=0Fh: 80x25 colour text");
    check(GetProcAddress(GetModuleHandle("USER"), MAKEINTRESOURCE(275)) != NULL && GetProcAddress(GetModuleHandle("USER"), MAKEINTRESOURCE(321)) != NULL,
          "USER's RepaintScreen and SetEventHook (Turbo Debugger)");
}

/* Windows DLLs that import each other (Resource Workshop's WORKLIB1 and
 * WORKLIB2); a DLL's instance handle; a DLL whose file is replaced. */
static int copy_file(const char *from, const char *to)
{
    static char buf[4096];
    int in, o, n, ok = 1;

    in = open(from, O_RDONLY | O_BINARY);
    o = open(to, O_WRONLY | O_CREAT | O_TRUNC | O_BINARY, 0666);
    if (in < 0 || o < 0)
        ok = 0;
    while (ok && (n = read(in, buf, sizeof buf)) > 0)
        ok = write(o, buf, n) == n;
    if (in >= 0)
        close(in);
    if (o >= 0)
        close(o);
    return ok;
}

static void dll_cycle_tests(void)
{
    HINSTANCE d;
    int (FAR PASCAL *f)(void);
    UINT (FAR PASCAL *m)(void);
    HINSTANCE (FAR PASCAL *inst)(void);

    d = LoadLibrary("W16CYC1.DLL");
    f = d > HINSTANCE_ERROR ? (int (FAR PASCAL *)(void))GetProcAddress(d, "W16CYC1VALUE") : NULL;
    m = d > HINSTANCE_ERROR ? (UINT (FAR PASCAL *)(void))GetProcAddress(d, "W16CYC1MESSAGE") : NULL;
    check(f && f() == 11, "two DLLs that import each other load and call each other");
    check(m && m() >= 0xc000, "a DLL's LibMain registers a window message");
    if (d > HINSTANCE_ERROR)
        FreeLibrary(d);

    d = LoadLibrary("W16DLL.DLL");
    inst = d > HINSTANCE_ERROR ? (HINSTANCE (FAR PASCAL *)(void))GetProcAddress(d, "W16DLLINSTANCE") : NULL;
    check(inst && inst() == d, "a DLL's LibMain gets the instance handle LoadLibrary returns (a VBX's models are found by it)");
    check((WORD)d < 0x8000 && SELECTOROF(inst) < 0x8000, "a DLL's selectors are below 8000h");
    if (d > HINSTANCE_ERROR)
        FreeLibrary(d);

    /* Replace W16DLL.DLL with W16D43.DLL (the same module, value 43), as
     * Delphi rebuilds COMPLIB.DCL, and load it again. */
    if (copy_file("C:\\RS2\\W16DLL.DLL", "C:\\W16DLL.SAV") && copy_file("C:\\RS2\\W16D43.DLL", "C:\\RS2\\W16DLL.DLL")) {
        d = LoadLibrary("W16DLL.DLL");
        f = d > HINSTANCE_ERROR ? (int (FAR PASCAL *)(void))GetProcAddress(d, "W16DLLVALUE") : NULL;
        check(f && f() == 43, "a DLL whose file was replaced loads from the new file");
        if (d > HINSTANCE_ERROR)
            FreeLibrary(d);
        copy_file("C:\\W16DLL.SAV", "C:\\RS2\\W16DLL.DLL");
        unlink("C:\\W16DLL.SAV");
    } else {
        check(0, "a DLL whose file was replaced loads from the new file (copying it)");
    }
}

/* ---- Stage 2's rest: MDI, bitmaps, regions, mapping modes, fonts,
 * DLLs and profiles in depth ---- */

static HWND mdi2_client;
static int mdi2_sizes, mdi2_maxsize;

LONG FAR PASCAL _export Mdi2FrameProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    if (msg == WM_CREATE) {
        CLIENTCREATESTRUCT ccs;

        /* As Multipad makes its client: no size, scroll bars. */
        ccs.hWindowMenu = GetSubMenu(GetMenu(hwnd), 1);
        ccs.idFirstChild = 2000;
        mdi2_client = CreateWindow("MDICLIENT", NULL, WS_CHILD | WS_CLIPCHILDREN | WS_VSCROLL | WS_HSCROLL, 0, 0, 0, 0, hwnd,
                                   (HMENU)0xCAC, ((CREATESTRUCT FAR *)lp)->hInstance, (LPSTR)&ccs);
        ShowWindow(mdi2_client, SW_SHOW);
        return 0;
    }
    return DefFrameProc(hwnd, mdi2_client, msg, wp, lp);
}

LONG FAR PASCAL _export Mdi2ChildProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    if (msg == WM_SIZE) {
        mdi2_sizes++;
        if (wp == SIZE_MAXIMIZED)
            mdi2_maxsize++;
    }
    return DefMDIChildProc(hwnd, msg, wp, lp);
}

static HWND mdi2_child(HINSTANCE hInst, LPCSTR title, DWORD style)
{
    MDICREATESTRUCT mcs;

    memset(&mcs, 0, sizeof mcs);
    mcs.szClass = "W16M2Child";
    mcs.szTitle = title;
    mcs.hOwner = hInst;
    mcs.x = mcs.y = mcs.cx = mcs.cy = CW_USEDEFAULT;
    mcs.style = style;
    return (HWND)LOWORD(SendMessage(mdi2_client, WM_MDICREATE, 0, (LPARAM)(LPMDICREATESTRUCT)&mcs));
}

static void mdi2_tests(HINSTANCE hInst)
{
    WNDCLASS wc;
    HWND frame, c1, c2, c3;
    HMENU bar, win, bar2;
    RECT r, rc, r2, r3;
    POINT p, q;
    char buf[80];
    DWORD act;

    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = Mdi2FrameProc;
    wc.hInstance = hInst;
    wc.hbrBackground = COLOR_APPWORKSPACE + 1;
    wc.lpszClassName = "W16M2Frame";
    RegisterClass(&wc);
    wc.lpfnWndProc = Mdi2ChildProc;
    wc.hbrBackground = GetStockObject(WHITE_BRUSH);
    wc.lpszClassName = "W16M2Child";
    RegisterClass(&wc);
    bar = CreateMenu();
    win = CreatePopupMenu();
    AppendMenu(win, MF_STRING, 1, "&Tile");
    AppendMenu(win, MF_STRING, 2, "&Cascade");
    AppendMenu(bar, MF_POPUP, (UINT)CreatePopupMenu(), "&File");
    AppendMenu(bar, MF_POPUP, (UINT)win, "&Window");
    frame = CreateWindow("W16M2Frame", "MDI2", WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN, 20, 20, 500, 360, NULL, bar, hInst, NULL);
    ShowWindow(frame, SW_SHOW);
    UpdateWindow(frame);
    check(mdi2_client != 0, "MDI: a client made in the frame's WM_CREATE (no size, scroll bars)");
    GetClientRect(frame, &r);
    GetClientRect(mdi2_client, &rc);
    check(rc.right == r.right && rc.bottom == r.bottom, "MDI: DefFrameProc's WM_SIZE sizes the client to the frame's client area");

    /* A child made maximized (Multipad's first). */
    mdi2_maxsize = 0;
    c1 = mdi2_child(hInst, "Doc1", WS_MAXIMIZE);
    act = SendMessage(mdi2_client, WM_MDIGETACTIVE, 0, 0);
    check(c1 && IsZoomed(c1) && LOWORD(act) == (WORD)c1 && HIWORD(act) == 1, "MDI: a child made with WS_MAXIMIZE is maximized (WM_MDIGETACTIVE says so)");
    check(mdi2_maxsize > 0, "MDI: the maximized child gets WM_SIZE with SIZE_MAXIMIZED");
    GetClientRect(c1, &r);
    GetClientRect(mdi2_client, &rc);
    wsprintf(line, "MDI: a maximized child's client area is the MDI client's (%d x %d, %d x %d)", r.right, r.bottom, rc.right, rc.bottom);
    check(r.right == rc.right && r.bottom == rc.bottom, line);
    p.x = p.y = q.x = q.y = 0;
    ClientToScreen(c1, &p);
    ClientToScreen(mdi2_client, &q);
    check(p.x == q.x && p.y == q.y, "MDI: its title bar and border are outside the client");
    GetWindowText(frame, buf, sizeof buf);
    check(lstrcmp(buf, "MDI2 - [Doc1]") == 0, "MDI: the frame's title is \"Frame - [Child]\"");
    check(GetMenuItemCount(bar) == 4 && GetSubMenu(bar, 0) != 0 && GetMenuItemID(bar, 3) == SC_RESTORE,
          "MDI: the menu bar gets the child's system menu and a restore button");
    check(GetSubMenu(bar, 2) == win, "MDI: the program's pop-ups move one place on");
    c2 = mdi2_child(hInst, "Doc2", 0);
    check(c2 && IsZoomed(c2) && !IsZoomed(c1), "MDI: a new child is maximized while one is (the other is restored)");
    GetWindowText(frame, buf, sizeof buf);
    check(lstrcmp(buf, "MDI2 - [Doc2]") == 0 && GetMenuItemCount(bar) == 4, "MDI: the title follows the maximized child, the menu items stay");
    SendMessage(mdi2_client, WM_MDIACTIVATE, (WPARAM)c1, 0);
    check(IsZoomed(c1) && !IsZoomed(c2), "MDI: activating another child maximizes it instead");
    SetWindowText(c1, "Renamed");
    GetWindowText(frame, buf, sizeof buf);
    check(lstrcmp(buf, "MDI2 - [Renamed]") == 0, "MDI: a maximized child's new title is in the frame's");
    GetMenuString(win, 2000, buf, sizeof buf, MF_BYCOMMAND);
    check(lstrcmp(buf, "&1 Renamed") == 0, "MDI: and in the Window menu");
    SetWindowText(frame, "MDI2b");
    GetWindowText(frame, buf, sizeof buf);
    check(lstrcmp(buf, "MDI2b - [Renamed]") == 0, "MDI: the frame's own new title keeps the child's");
    /* The restore button: WM_COMMAND SC_RESTORE to the frame. */
    SendMessage(frame, WM_COMMAND, SC_RESTORE, 0);
    GetWindowText(frame, buf, sizeof buf);
    check(!IsZoomed(c1) && GetMenuItemCount(bar) == 2 && lstrcmp(buf, "MDI2b") == 0,
          "MDI: the restore button restores the child, the menu bar and the title");
    check(HIWORD(SendMessage(mdi2_client, WM_MDIGETACTIVE, 0, 0)) == 0, "MDI: WM_MDIGETACTIVE: not maximized");
    SendMessage(mdi2_client, WM_MDIMAXIMIZE, (WPARAM)c2, 0);
    check(IsZoomed(c2) && LOWORD(SendMessage(mdi2_client, WM_MDIGETACTIVE, 0, 0)) == (WORD)c2, "MDI: WM_MDIMAXIMIZE");
    SendMessage(mdi2_client, WM_MDIRESTORE, (WPARAM)c2, 0);
    check(!IsZoomed(c2) && GetMenuItemCount(bar) == 2, "MDI: WM_MDIRESTORE");
    SendMessage(c2, WM_SYSCOMMAND, SC_MAXIMIZE, 0);
    check(IsZoomed(c2), "MDI: WM_SYSCOMMAND SC_MAXIMIZE to a child");
    /* The program changes its menu (Multipad's short menu). */
    bar2 = CreateMenu();
    AppendMenu(bar2, MF_POPUP, (UINT)CreatePopupMenu(), "&Short");
    SendMessage(mdi2_client, WM_MDISETMENU, 0, MAKELONG(bar2, 0));
    check(GetMenu(frame) == bar2 && GetMenuItemCount(bar2) == 3 && GetMenuItemCount(bar) == 2, "MDI: WM_MDISETMENU moves the maximized child's items");
    SendMessage(mdi2_client, WM_MDISETMENU, 0, MAKELONG(bar, 0));
    check(GetMenuItemCount(bar) == 4 && GetMenuItemCount(bar2) == 1, "MDI: and back");
    DestroyMenu(bar2);
    /* Minimized: an icon at the bottom of the client. */
    SendMessage(c2, WM_SYSCOMMAND, SC_RESTORE, 0);
    SendMessage(c1, WM_SYSCOMMAND, SC_MINIMIZE, 0);
    GetWindowRect(c1, &r);
    GetWindowRect(mdi2_client, &rc);
    wsprintf(line, "MDI: a minimized child is an icon at the bottom of the client (%d,%d-%d,%d in %d,%d-%d,%d)", r.left, r.top, r.right,
             r.bottom, rc.left, rc.top, rc.right, rc.bottom);
    check(IsIconic(c1) && r.left >= rc.left && r.right <= rc.right && r.bottom <= rc.bottom && r.top > rc.top + (rc.bottom - rc.top) / 2, line);
    check(ArrangeIconicWindows(mdi2_client) > 0 && IsIconic(c1), "MDI: ArrangeIconicWindows");
    SendMessage(mdi2_client, WM_MDIICONARRANGE, 0, 0);
    /* Tiling: the icons keep their row. */
    c3 = mdi2_child(hInst, "Doc3", 0);
    SendMessage(mdi2_client, WM_MDITILE, MDITILE_VERTICAL, 0);
    GetWindowRect(c2, &r2);
    GetWindowRect(c3, &r3);
    check(r2.top == r3.top && r2.left != r3.left && r2.bottom < r.top + 2, "MDI: WM_MDITILE (side by side, above the icons)");
    SendMessage(mdi2_client, WM_MDITILE, MDITILE_HORIZONTAL, 0);
    GetWindowRect(c2, &r2);
    GetWindowRect(c3, &r3);
    check(r2.left == r3.left && r2.top != r3.top, "MDI: WM_MDITILE MDITILE_HORIZONTAL (one above the other)");
    check(SendMessage(mdi2_client, WM_MDICASCADE, 0, 0) != 0, "MDI: WM_MDICASCADE");
    GetWindowRect(c2, &r2);
    GetWindowRect(c3, &r3);
    check(r3.left > r2.left && r3.top > r2.top && IsIconic(c1), "MDI: cascaded, each one lower right of the one before");
    SendMessage(mdi2_client, WM_MDINEXT, 0, 0);
    act = SendMessage(mdi2_client, WM_MDIGETACTIVE, 0, 0);
    check(LOWORD(act) != (WORD)c3, "MDI: WM_MDINEXT");
    /* The Window menu: a click on an icon's entry restores it. */
    SendMessage(frame, WM_COMMAND, 2000, 0);
    check(!IsIconic(c1) && LOWORD(SendMessage(mdi2_client, WM_MDIGETACTIVE, 0, 0)) == (WORD)c1, "MDI: the Window menu's entry restores and activates an icon");
    /* The maximized child goes: the next one is maximized. */
    SendMessage(mdi2_client, WM_MDIMAXIMIZE, (WPARAM)c1, 0);
    SendMessage(mdi2_client, WM_MDIDESTROY, (WPARAM)c1, 0);
    act = SendMessage(mdi2_client, WM_MDIGETACTIVE, 0, 0);
    check(!IsWindow(c1) && LOWORD(act) != 0 && HIWORD(act) == 1 && GetMenuItemCount(bar) == 4, "MDI: when the maximized child goes, the next one is maximized");
    DestroyWindow(frame);
}

/* A memory DC with a 24-bit bitmap of w x h, filled white. */
static HDC mem_dc(int w, int h, HBITMAP *bm, HBITMAP *old)
{
    HDC dc = CreateCompatibleDC(NULL);

    *bm = CreateBitmap(w, h, 1, 24, NULL);
    *old = SelectObject(dc, *bm);
    PatBlt(dc, 0, 0, w, h, WHITENESS);
    return dc;
}

static void free_dc(HDC dc, HBITMAP bm, HBITMAP old)
{
    SelectObject(dc, old);
    DeleteObject(bm);
    DeleteDC(dc);
}

#define RED RGB(255, 0, 0)
#define GREEN RGB(0, 255, 0)
#define BLUE RGB(0, 0, 255)
#define WHITE RGB(255, 255, 255)
#define BLACK RGB(0, 0, 0)

/* Bitmaps, brushes, raster operations and DIBs. */
static void gdi2_tests(HINSTANCE hInst)
{
    HDC dc, src;
    HBITMAP bm, old, sbm, sold, mono, b;
    HBRUSH br, obr;
    BITMAP info;
    LOGBRUSH lb;
    RECT r;
    int i;
    static BYTE checker[16] = { 0xaa, 0, 0x55, 0, 0xaa, 0, 0x55, 0, 0xaa, 0, 0x55, 0, 0xaa, 0, 0x55, 0 };
    /* A 4 x 2 DIB, 8 bits: indexes 0..3 into red, green, blue, white. */
    struct { BITMAPINFOHEADER h; RGBQUAD c[4]; } bi8;
    static BYTE pix8[8] = { 0, 1, 2, 3, 3, 2, 1, 0 };
    /* 2 x 2, 4 bits: rows bottom-up, 4-byte aligned. */
    struct { BITMAPINFOHEADER h; RGBQUAD c[16]; } bi4;
    static BYTE pix4[8] = { 0x01, 0, 0, 0, 0x23, 0, 0, 0 };
    /* 8 x 8, 1 bit, for a DIB pattern brush: a packed DIB in a global block. */
    HGLOBAL g;
    BYTE FAR *pd;

    dc = mem_dc(32, 32, &bm, &old);
    src = mem_dc(32, 32, &sbm, &sold);
    /* Raster operations on 24-bit pels are bitwise. */
    PatBlt(dc, 0, 0, 32, 32, BLACKNESS);
    br = CreateSolidBrush(RGB(0x0f, 0xf0, 0x33));
    obr = SelectObject(dc, br);
    PatBlt(dc, 0, 0, 4, 4, PATCOPY);
    PatBlt(dc, 0, 0, 2, 2, PATINVERT);
    check(GetPixel(dc, 3, 3) == RGB(0x0f, 0xf0, 0x33) && GetPixel(dc, 1, 1) == BLACK, "PatBlt PATCOPY, PATINVERT (XOR with the brush, not an inversion)");
    PatBlt(dc, 4, 0, 2, 2, DSTINVERT);
    check(GetPixel(dc, 4, 0) == WHITE, "PatBlt DSTINVERT");
    SelectObject(dc, obr);
    DeleteObject(br);
    /* Destination 0x00ff00 (green), source 0xff00ff... per channel. */
    for (i = 0; i < 7; i++) {
        static DWORD rops[7] = { SRCPAINT, SRCAND, SRCINVERT, NOTSRCCOPY, MERGEPAINT, SRCERASE, NOTSRCERASE };
        static COLORREF want[7];
        COLORREF d = RGB(0x00, 0xf0, 0x3c), s = RGB(0x0f, 0xff, 0x30);
        want[0] = d | s;
        want[1] = d & s;
        want[2] = d ^ s;
        want[3] = ~s & 0xffffff;
        want[4] = (~s | d) & 0xffffff;
        want[5] = s & ~d & 0xffffff;
        want[6] = ~(s | d) & 0xffffff;
        SetPixel(dc, 10, 10, d);
        SetPixel(src, 0, 0, s);
        BitBlt(dc, 10, 10, 1, 1, src, 0, 0, rops[i]);
        wsprintf(line, "BitBlt ROP %08lx: %06lx (%06lx)", rops[i], GetPixel(dc, 10, 10), want[i]);
        check(GetPixel(dc, 10, 10) == want[i], line);
    }
    /* Hatched brushes: lines in the brush's colour, the rest the
     * background colour (OPAQUE) or left alone (TRANSPARENT). */
    PatBlt(dc, 0, 0, 32, 32, WHITENESS);
    br = CreateHatchBrush(HS_CROSS, RED);
    SetBkColor(dc, BLUE);
    SetRect(&r, 0, 0, 16, 16);
    FillRect(dc, &r, br);
    {
        int lines = 0, bk = 0, x, y;
        for (y = 0; y < 16; y++)
            for (x = 0; x < 16; x++) {
                COLORREF c = GetPixel(dc, x, y);
                lines += c == RED;
                bk += c == BLUE;
            }
        wsprintf(line, "CreateHatchBrush HS_CROSS: a grid in its colour on the background colour (%d, %d of 256)", lines, bk);
        check(lines >= 50 && lines <= 80 && bk + lines == 256, line);
    }
    SetBkMode(dc, TRANSPARENT);
    PatBlt(dc, 16, 0, 16, 16, WHITENESS);
    SetRect(&r, 16, 0, 32, 16);
    FillRect(dc, &r, br);
    {
        int lines = 0, white = 0, x, y;
        for (y = 0; y < 16; y++)
            for (x = 16; x < 32; x++) {
                COLORREF c = GetPixel(dc, x, y);
                lines += c == RED;
                white += c == WHITE;
            }
        check(lines >= 50 && white + lines == 256, "a hatched brush in TRANSPARENT mode leaves the gaps");
    }
    SetBkMode(dc, OPAQUE);
    check(GetObject(br, sizeof lb, &lb) == sizeof lb && lb.lbStyle == BS_HATCHED && lb.lbHatch == HS_CROSS && lb.lbColor == RED,
          "GetObject of a hatched brush");
    DeleteObject(br);
    lb.lbStyle = BS_HATCHED;
    lb.lbColor = GREEN;
    lb.lbHatch = HS_VERTICAL;
    br = CreateBrushIndirect(&lb);
    obr = SelectObject(dc, br);
    PatBlt(dc, 0, 16, 16, 16, PATCOPY);
    {
        int x, cols = 0;
        for (x = 0; x < 16; x++)
            cols += GetPixel(dc, x, 20) == GREEN && GetPixel(dc, x, 27) == GREEN;
        check(cols == 2, "CreateBrushIndirect BS_HATCHED HS_VERTICAL: two columns in 16 pels (PatBlt PATCOPY)");
    }
    SelectObject(dc, obr);
    DeleteObject(br);
    /* A pattern brush of a monochrome bitmap: 1 bits in the background
     * colour, 0 bits in the text colour. */
    mono = CreateBitmap(8, 8, 1, 1, checker);
    br = CreatePatternBrush(mono);
    SetTextColor(dc, RED);
    SetBkColor(dc, BLUE);
    SetRect(&r, 0, 0, 16, 16);
    FillRect(dc, &r, br);
    wsprintf(line, "CreatePatternBrush of a monochrome bitmap: 1 bits the background colour, 0 bits the text colour (%06lx %06lx)",
             GetPixel(dc, 0, 0), GetPixel(dc, 1, 0));
    check(GetPixel(dc, 0, 0) == BLUE && GetPixel(dc, 1, 0) == RED && GetPixel(dc, 0, 1) == RED && GetPixel(dc, 9, 1) == BLUE, line);
    check(GetObject(br, sizeof lb, &lb) == sizeof lb && lb.lbStyle == BS_PATTERN, "GetObject of a pattern brush");
    DeleteObject(br);
    DeleteObject(mono);
    /* A colour pattern: one green pel in white. */
    b = CreateBitmap(8, 8, 1, 24, NULL);
    sold = SelectObject(src, b);
    PatBlt(src, 0, 0, 8, 8, WHITENESS);
    SetPixel(src, 2, 3, GREEN);
    SelectObject(src, sbm);
    br = CreatePatternBrush(b);
    SetRect(&r, 0, 0, 16, 16);
    FillRect(dc, &r, br);
    check(GetPixel(dc, 2, 3) == GREEN && GetPixel(dc, 10, 11) == GREEN && GetPixel(dc, 3, 3) == WHITE, "CreatePatternBrush of a colour bitmap, repeated every 8 pels");
    obr = SelectObject(dc, br);
    Ellipse(dc, 16, 16, 32, 32);
    check(GetPixel(dc, 26, 27) == GREEN && GetPixel(dc, 16, 16) == WHITE, "Ellipse with a pattern brush");
    SelectObject(dc, obr);
    DeleteObject(br);
    DeleteObject(b);
    /* CreateDIBPatternBrush: a packed DIB, 1 bit, red and blue. */
    g = GlobalAlloc(GMEM_MOVEABLE, sizeof(BITMAPINFOHEADER) + 8 + 32);
    pd = GlobalLock(g);
    _fmemset(pd, 0, sizeof(BITMAPINFOHEADER) + 8 + 32);
    ((BITMAPINFOHEADER FAR *)pd)->biSize = sizeof(BITMAPINFOHEADER);
    ((BITMAPINFOHEADER FAR *)pd)->biWidth = 8;
    ((BITMAPINFOHEADER FAR *)pd)->biHeight = 8;
    ((BITMAPINFOHEADER FAR *)pd)->biPlanes = 1;
    ((BITMAPINFOHEADER FAR *)pd)->biBitCount = 1;
    pd[40 + 2] = 0xff;           /* colour 0 red */
    pd[44 + 0] = 0xff;           /* colour 1 blue */
    for (i = 0; i < 8; i++)
        pd[48 + i * 4] = 0xf0;   /* the left half 1 (blue) */
    GlobalUnlock(g);
    br = CreateDIBPatternBrush(g, DIB_RGB_COLORS);
    SetRect(&r, 0, 0, 16, 8);
    FillRect(dc, &r, br);
    check(br != 0 && GetPixel(dc, 1, 1) == BLUE && GetPixel(dc, 6, 1) == RED && GetPixel(dc, 9, 5) == BLUE, "CreateDIBPatternBrush");
    DeleteObject(br);
    GlobalFree(g);
    /* RoundRect: the corners stay outside. */
    PatBlt(dc, 0, 0, 32, 32, WHITENESS);
    obr = SelectObject(dc, GetStockObject(BLACK_BRUSH));
    RoundRect(dc, 0, 0, 30, 30, 16, 16);
    check(GetPixel(dc, 0, 0) == WHITE && GetPixel(dc, 15, 15) == BLACK && GetPixel(dc, 15, 1) == BLACK, "RoundRect has round corners");
    SelectObject(dc, obr);

    /* DIBs: 8 bits with a colour table, stretched. */
    memset(&bi8, 0, sizeof bi8);
    bi8.h.biSize = sizeof bi8.h;
    bi8.h.biWidth = 4;
    bi8.h.biHeight = 2;
    bi8.h.biPlanes = 1;
    bi8.h.biBitCount = 8;
    bi8.h.biClrUsed = 4;
    bi8.c[0].rgbRed = 255;
    bi8.c[1].rgbGreen = 255;
    bi8.c[2].rgbBlue = 255;
    bi8.c[3].rgbRed = bi8.c[3].rgbGreen = bi8.c[3].rgbBlue = 255;
    PatBlt(dc, 0, 0, 32, 32, BLACKNESS);
    StretchDIBits(dc, 0, 0, 8, 4, 0, 0, 4, 2, pix8, (BITMAPINFO FAR *)&bi8, DIB_RGB_COLORS, SRCCOPY);
    /* The DIB's first row (pix8[0..3]) is its bottom one. */
    wsprintf(line, "StretchDIBits of an 8-bit DIB, doubled (%06lx %06lx %06lx)", GetPixel(dc, 0, 3), GetPixel(dc, 3, 3), GetPixel(dc, 1, 0));
    check(GetPixel(dc, 0, 3) == RED && GetPixel(dc, 3, 3) == GREEN && GetPixel(dc, 1, 0) == WHITE && GetPixel(dc, 7, 0) == RED, line);
    SetDIBitsToDevice(dc, 10, 10, 4, 2, 0, 0, 0, 2, pix8, (BITMAPINFO FAR *)&bi8, DIB_RGB_COLORS);
    check(GetPixel(dc, 10, 11) == RED && GetPixel(dc, 12, 11) == BLUE && GetPixel(dc, 10, 10) == WHITE, "SetDIBitsToDevice of an 8-bit DIB");
    memset(&bi4, 0, sizeof bi4);
    bi4.h.biSize = sizeof bi4.h;
    bi4.h.biWidth = 2;
    bi4.h.biHeight = 2;
    bi4.h.biPlanes = 1;
    bi4.h.biBitCount = 4;
    bi4.c[0].rgbRed = 255;
    bi4.c[1].rgbGreen = 255;
    bi4.c[2].rgbBlue = 255;
    bi4.c[3].rgbRed = 255;
    bi4.c[3].rgbBlue = 255;
    SetDIBitsToDevice(dc, 20, 20, 2, 2, 0, 0, 0, 2, pix4, (BITMAPINFO FAR *)&bi4, DIB_RGB_COLORS);
    check(GetPixel(dc, 20, 21) == RED && GetPixel(dc, 21, 21) == GREEN && GetPixel(dc, 20, 20) == BLUE && GetPixel(dc, 21, 20) == RGB(255, 0, 255),
          "SetDIBitsToDevice of a 4-bit DIB");
    b = CreateDIBitmap(dc, &bi8.h, CBM_INIT, pix8, (BITMAPINFO FAR *)&bi8, DIB_RGB_COLORS);
    check(b != 0 && GetObject(b, sizeof info, &info) == sizeof info && info.bmWidth == 4 && info.bmHeight == 2 && info.bmBitsPixel > 1,
          "CreateDIBitmap CBM_INIT: a colour bitmap of the DC's format");
    sold = SelectObject(src, b);
    check(GetPixel(src, 0, 1) == RED && GetPixel(src, 2, 0) == GREEN, "CreateDIBitmap's pels");
    SelectObject(src, sbm);
    {
        /* GetDIBits back as 8 bits: a colour table and indexes. */
        struct { BITMAPINFOHEADER h; RGBQUAD c[256]; } out;
        BYTE px[8];
        memset(&out, 0, sizeof out);
        out.h.biSize = sizeof out.h;
        out.h.biWidth = 4;
        out.h.biHeight = 2;
        out.h.biPlanes = 1;
        out.h.biBitCount = 8;
        i = GetDIBits(dc, b, 0, 2, px, (BITMAPINFO FAR *)&out, DIB_RGB_COLORS);
        check(i == 2 && out.c[px[0]].rgbRed == 255 && out.c[px[0]].rgbGreen == 0 && out.c[px[1]].rgbGreen == 255 && out.c[px[3]].rgbBlue == 255,
              "GetDIBits as an 8-bit DIB: the colour table and indexes");
    }
    DeleteObject(b);
    /* Resources: a 4-bit bitmap, a cursor, an icon. */
    b = LoadBitmap(hInst, MAKEINTRESOURCE(IDB_QUAD));
    check(b != 0 && GetObject(b, sizeof info, &info) == sizeof info && info.bmWidth == 16 && info.bmHeight == 16, "LoadBitmap of a BITMAP resource");
    sold = SelectObject(src, b);
    check(GetPixel(src, 2, 2) == RED && GetPixel(src, 12, 2) == GREEN && GetPixel(src, 2, 12) == BLUE && GetPixel(src, 12, 12) == WHITE,
          "the resource bitmap's pels (DIB rows bottom-up)");
    StretchBlt(dc, 0, 0, 4, 4, src, 0, 0, 16, 16, SRCCOPY);
    check(GetPixel(dc, 0, 0) == RED && GetPixel(dc, 3, 3) == WHITE, "StretchBlt shrinks");
    SelectObject(src, sbm);
    DeleteObject(b);
    {
        HCURSOR c = LoadCursor(hInst, MAKEINTRESOURCE(IDCUR_TEST));
        HCURSOR prev;
        HICON ic = LoadIcon(hInst, MAKEINTRESOURCE(IDI_TEST));
        check(c != 0 && c != LoadCursor(NULL, IDC_ARROW), "LoadCursor of a CURSOR resource");
        prev = SetCursor(c);
        check(SetCursor(prev) == c, "SetCursor of it");
        check(ic != 0 && GetSystemMetrics(SM_CXICON) == 32, "LoadIcon of an ICON resource");
        PatBlt(dc, 0, 0, 32, 32, WHITENESS);
        DrawIcon(dc, 0, 0, ic);
        {
            int x, y, drawn = 0;
            for (y = 0; y < 32; y += 2)
                for (x = 0; x < 32; x += 2)
                    drawn += GetPixel(dc, x, y) != WHITE;
            check(drawn > 20, "DrawIcon draws the icon");
        }
    }
    free_dc(src, sbm, sold == sbm ? sold : sbm);
    free_dc(dc, bm, old);
}

/* Regions in depth. */
static int rgn_paints;
static RECT rgn_update;

LONG FAR PASCAL _export RgnWndProc(HWND h, UINT msg, WPARAM wp, LPARAM lp)
{
    if (msg == WM_PAINT) {
        PAINTSTRUCT ps;
        BeginPaint(h, &ps);
        rgn_update = ps.rcPaint;
        rgn_paints++;
        EndPaint(h, &ps);
        return 0;
    }
    return DefWindowProc(h, msg, wp, lp);
}

static void rgn2_tests(HINSTANCE hInst)
{
    HDC dc;
    HBITMAP bm, old;
    HRGN a, b, c;
    RECT r;
    WNDCLASS wc;
    HWND w;
    static POINT tri[3] = { { 0, 0 }, { 20, 0 }, { 0, 20 } };
    MSG msg;

    a = CreatePolygonRgn(tri, 3, ALTERNATE);
    check(a && PtInRegion(a, 2, 2) && !PtInRegion(a, 18, 18) && GetRgnBox(a, &r) && r.right <= 20 && r.bottom <= 20, "CreatePolygonRgn");
    b = CreateRoundRectRgn(0, 0, 20, 20, 10, 10);
    check(b && !PtInRegion(b, 0, 0) && PtInRegion(b, 10, 10) && PtInRegion(b, 10, 0), "CreateRoundRectRgn");
    c = CreateRectRgn(0, 0, 0, 0);
    check(CombineRgn(c, a, b, RGN_XOR) == COMPLEXREGION && PtInRegion(c, 15, 15) && !PtInRegion(c, 5, 5), "CombineRgn RGN_XOR");
    check(CombineRgn(c, a, NULL, RGN_COPY) != ERROR && EqualRgn(c, a), "CombineRgn RGN_COPY");
    SetRectRgn(c, 5, 5, 10, 10);
    SetRect(&r, 8, 8, 20, 20);
    check(RectInRegion(c, &r) && GetRgnBox(c, &r) == SIMPLEREGION && r.left == 5, "SetRectRgn, RectInRegion, GetRgnBox");
    DeleteObject(a);
    DeleteObject(b);

    /* Drawing through a clip region. */
    dc = mem_dc(32, 32, &bm, &old);
    a = CreateEllipticRgn(0, 0, 32, 32);
    check(SelectClipRgn(dc, a) == COMPLEXREGION, "SelectClipRgn: the region's type");
    PatBlt(dc, 0, 0, 32, 32, BLACKNESS);
    check(GetPixel(dc, 0, 0) == WHITE && GetPixel(dc, 16, 16) == BLACK, "SelectClipRgn clips drawing to the ellipse");
    check(GetClipBox(dc, &r) == COMPLEXREGION && r.left >= 0 && r.right <= 32 && r.right > 16, "GetClipBox of a clip region");
    OffsetClipRgn(dc, 16, 0);
    PatBlt(dc, 0, 0, 32, 32, WHITENESS);
    check(GetPixel(dc, 4, 16) == BLACK && GetPixel(dc, 20, 16) == WHITE, "OffsetClipRgn moves it");
    SelectClipRgn(dc, NULL);
    PatBlt(dc, 0, 0, 32, 32, WHITENESS);
    SetRectRgn(a, 4, 4, 8, 8);
    PaintRgn(dc, a);
    check(GetPixel(dc, 5, 5) == WHITE, "PaintRgn with the DC's brush (white)");
    FillRgn(dc, a, GetStockObject(BLACK_BRUSH));
    check(GetPixel(dc, 5, 5) == BLACK && GetPixel(dc, 9, 9) == WHITE, "FillRgn");
    InvertRgn(dc, a);
    check(GetPixel(dc, 5, 5) == WHITE, "InvertRgn");
    SetRectRgn(a, 10, 10, 20, 20);
    FrameRgn(dc, a, GetStockObject(BLACK_BRUSH), 2, 2);
    check(GetPixel(dc, 10, 10) == BLACK && GetPixel(dc, 11, 15) == BLACK && GetPixel(dc, 15, 15) == WHITE, "FrameRgn");
    b = CreateEllipticRgn(0, 0, 30, 30);
    PatBlt(dc, 0, 0, 32, 32, WHITENESS);
    FrameRgn(dc, b, GetStockObject(BLACK_BRUSH), 1, 1);
    check(GetPixel(dc, 15, 15) == WHITE && GetPixel(dc, 15, 0) == BLACK, "FrameRgn of an ellipse frames it (not fills it)");
    DeleteObject(b);
    free_dc(dc, bm, old);

    /* InvalidateRgn invalidates the region, not the whole window. */
    memset(&wc, 0, sizeof wc);
    wc.lpfnWndProc = RgnWndProc;
    wc.hInstance = hInst;
    wc.hbrBackground = GetStockObject(WHITE_BRUSH);
    wc.lpszClassName = "W16Rgn";
    RegisterClass(&wc);
    w = CreateWindow("W16Rgn", "W16TEST regions", WS_OVERLAPPEDWINDOW | WS_VISIBLE, 60, 60, 300, 200, NULL, NULL, hInst, NULL);
    UpdateWindow(w);
    SetRectRgn(a, 10, 20, 50, 60);
    InvalidateRgn(w, a, TRUE);
    check(GetUpdateRect(w, &r, FALSE) && r.left == 10 && r.top == 20 && r.right == 50 && r.bottom == 60, "InvalidateRgn: the update area is the region");
    b = CreateRectRgn(0, 0, 0, 0);
    check(GetUpdateRgn(w, b, FALSE) != NULLREGION && GetRgnBox(b, &r) && r.left == 10 && r.bottom == 60, "GetUpdateRgn");
    rgn_paints = 0;
    UpdateWindow(w);
    check(rgn_paints == 1 && rgn_update.left == 10 && rgn_update.bottom == 60, "WM_PAINT's rcPaint is the region's box");
    InvalidateRgn(w, a, TRUE);
    ValidateRgn(w, a);
    check(!GetUpdateRect(w, &r, FALSE), "ValidateRgn");
    while (PeekMessage(&msg, w, 0, 0, PM_REMOVE))
        DispatchMessage(&msg);
    DestroyWindow(w);
    DeleteObject(a);
    DeleteObject(b);
    DeleteObject(c);
}

/* Mapping modes in depth. */
static void map2_tests(void)
{
    HDC dc;
    HBITMAP bm, old;
    POINT p;
    DWORD v;
    int dpi;

    dc = mem_dc(200, 200, &bm, &old);
    dpi = GetDeviceCaps(dc, LOGPIXELSX);
    SetMapMode(dc, MM_LOMETRIC);
    p.x = 254;
    p.y = -254;
    LPtoDP(dc, &p, 1);
    wsprintf(line, "MM_LOMETRIC: 25.4 mm is an inch, y goes up (%d, %d at %d dpi)", p.x, p.y, dpi);
    check(p.x == dpi && p.y == dpi, line);
    SetMapMode(dc, MM_HIENGLISH);
    p.x = 1000;
    p.y = -500;
    LPtoDP(dc, &p, 1);
    check(p.x == dpi && p.y == dpi / 2, "MM_HIENGLISH");
    SetMapMode(dc, MM_TWIPS);
    p.x = 1440;
    p.y = -1440;
    LPtoDP(dc, &p, 1);
    check(p.x == dpi && p.y == dpi, "MM_TWIPS");
    v = GetWindowExt(dc);
    check(LOWORD(v) == 1440 && GetMapMode(dc) == MM_TWIPS, "GetWindowExt of a fixed mode");
    /* Drawing in MM_LOMETRIC: a rectangle half an inch from the top. */
    SetMapMode(dc, MM_LOMETRIC);
    SelectObject(dc, GetStockObject(BLACK_BRUSH));
    Rectangle(dc, 127, -127, 254, -254);
    SetMapMode(dc, MM_TEXT);
    wsprintf(line, "Rectangle in MM_LOMETRIC lands where it should (%06lx %06lx)", GetPixel(dc, dpi * 3 / 4, dpi * 3 / 4), GetPixel(dc, dpi / 4, dpi / 4));
    check(GetPixel(dc, dpi * 3 / 4, dpi * 3 / 4) == BLACK && GetPixel(dc, dpi / 4, dpi / 4) == WHITE, line);
    /* MM_ISOTROPIC keeps units square: the larger viewport extent shrinks. */
    SetMapMode(dc, MM_ISOTROPIC);
    SetWindowExt(dc, 100, 100);
    SetViewportExt(dc, 200, 100);
    v = GetViewportExt(dc);
    wsprintf(line, "MM_ISOTROPIC: the viewport extent shrinks to keep units square (%d, %d)", LOWORD(v), HIWORD(v));
    check(LOWORD(v) == 100 && HIWORD(v) == 100, line);
    SetViewportExt(dc, 200, -100);
    v = GetViewportExt(dc);
    check((short)LOWORD(v) == 100 && (short)HIWORD(v) == -100, "MM_ISOTROPIC keeps the extents' signs");
    SetMapMode(dc, MM_ANISOTROPIC);
    SetWindowExt(dc, 10, 10);
    SetViewportExt(dc, 20, 30);
    ScaleViewportExt(dc, 1, 2, 2, 3);
    v = GetViewportExt(dc);
    check(LOWORD(v) == 10 && HIWORD(v) == 20, "ScaleViewportExt");
    ScaleWindowExt(dc, 2, 1, 1, 1);
    v = GetWindowExt(dc);
    check(LOWORD(v) == 20 && HIWORD(v) == 10, "ScaleWindowExt");
    SetViewportOrg(dc, 0, 0);
    OffsetViewportOrg(dc, 5, 6);
    OffsetWindowOrg(dc, 2, 3);
    v = GetViewportOrg(dc);
    check(LOWORD(v) == 5 && HIWORD(v) == 6 && LOWORD(GetWindowOrg(dc)) == 2, "OffsetViewportOrg, OffsetWindowOrg");
    p.x = 22;
    p.y = 13;
    LPtoDP(dc, &p, 1);
    check(p.x == 15 && p.y == 26, "MM_ANISOTROPIC with origins: LPtoDP");
    DPtoLP(dc, &p, 1);
    check(p.x == 22 && p.y == 13, "and DPtoLP back");
    MoveTo(dc, 4, 4);
    v = GetCurrentPosition(dc);
    check(LOWORD(v) == 4 && HIWORD(v) == 4, "GetCurrentPosition is logical");
    SetMapMode(dc, MM_TEXT);
    v = GetViewportExt(dc);
    check(LOWORD(v) == 1 && HIWORD(v) == 1 && LOWORD(GetViewportOrg(dc)) == 5, "MM_TEXT: unit extents, the origins stay");
    SetViewportOrg(dc, 0, 0);
    SetWindowOrg(dc, 0, 0);
    free_dc(dc, bm, old);
}

/* Fonts: enumeration and metrics with rs2's faces. */
static int fonts_seen, tt_seen, styles_seen, sizes_seen;
static char faces_seen[200];
static int last_type;
static char full_name[64];

int FAR PASCAL _export EnumFaceProc(LOGFONT FAR *lf, TEXTMETRIC FAR *tm, int type, LPARAM data)
{
    fonts_seen++;
    if (type & TRUETYPE_FONTTYPE)
        tt_seen++;
    if (lstrlen(faces_seen) + lstrlen(lf->lfFaceName) + 2 < sizeof faces_seen) {
        lstrcat(faces_seen, lf->lfFaceName);
        lstrcat(faces_seen, ";");
    }
    return tm->tmHeight > 0 && tm->tmAveCharWidth > 0 ? 1 : 0;
}

int FAR PASCAL _export EnumStyleProc(LOGFONT FAR *lf, TEXTMETRIC FAR *tm, int type, LPARAM data)
{
    ENUMLOGFONT FAR *elf = (ENUMLOGFONT FAR *)lf;
    NEWTEXTMETRIC FAR *ntm = (NEWTEXTMETRIC FAR *)tm;
    styles_seen++;
    last_type = type;
    if ((ntm->ntmFlags & (NTM_BOLD | NTM_ITALIC)) == NTM_BOLD)
        lstrcpy(full_name, (LPSTR)elf->elfFullName);
    return (int)data;
}

int FAR PASCAL _export EnumSizeProc(LOGFONT FAR *lf, TEXTMETRIC FAR *tm, int type, LPARAM data)
{
    sizes_seen++;
    return 1;
}

static void font2_tests(HWND hwnd)
{
    HDC dc = GetDC(hwnd);
    FONTENUMPROC fp;
    HFONT f, old, fb;
    TEXTMETRIC tm, tmb;
    char face[40];
    DWORD wi, ww;
    int w[3];

    fp = (FONTENUMPROC)MakeProcInstance((FARPROC)EnumFaceProc, GetWindowWord(hwnd, GWW_HINSTANCE));
    faces_seen[0] = 0;
    EnumFonts(dc, NULL, (OLDFONTENUMPROC)fp, 0);
    wsprintf(line, "EnumFonts: one font per face (%d, %d TrueType: %s)", fonts_seen, tt_seen, (LPSTR)faces_seen);
    check(fonts_seen >= 6 && tt_seen >= 3 && strstr(faces_seen, "Arial;") && strstr(faces_seen, "Courier New;") && strstr(faces_seen, "System;")
          && strstr(faces_seen, "MS Sans Serif;"), line);
    fonts_seen = 0;
    EnumFontFamilies(dc, NULL, fp, 0);
    check(fonts_seen >= 6, "EnumFontFamilies of all families");
    fp = (FONTENUMPROC)MakeProcInstance((FARPROC)EnumStyleProc, GetWindowWord(hwnd, GWW_HINSTANCE));
    EnumFontFamilies(dc, "Times New Roman", fp, 1);
    wsprintf(line, "EnumFontFamilies of a TrueType family: its four styles (%d, \"%s\")", styles_seen, (LPSTR)full_name);
    check(styles_seen == 4 && last_type == TRUETYPE_FONTTYPE && lstrcmp(full_name, "Times New Roman Bold") == 0, line);
    styles_seen = 0;
    check(EnumFontFamilies(dc, "Arial", fp, 0) == 0 && styles_seen == 1, "the callback's 0 ends the enumeration and is the result");
    fp = (FONTENUMPROC)MakeProcInstance((FARPROC)EnumSizeProc, GetWindowWord(hwnd, GWW_HINSTANCE));
    EnumFonts(dc, "MS Sans Serif", (OLDFONTENUMPROC)fp, 0);
    check(sizes_seen > 1, "EnumFonts of a raster face: its sizes");
    sizes_seen = 0;
    check(EnumFonts(dc, "No Such Face", (OLDFONTENUMPROC)fp, 0) == 1 && sizes_seen == 0, "EnumFonts of a face rs2 lacks: nothing");

    f = CreateFont(-20, 0, 0, 0, FW_NORMAL, 0, 0, 0, ANSI_CHARSET, 0, 0, 0, VARIABLE_PITCH | FF_SWISS, "Arial");
    old = SelectObject(dc, f);
    GetTextMetrics(dc, &tm);
    GetTextFace(dc, sizeof face, face);
    wsprintf(line, "CreateFont Arial -20: TEXTMETRIC (height %d, ascent %d, internal leading %d, average %d, pitch %02x)", tm.tmHeight, tm.tmAscent,
             tm.tmInternalLeading, tm.tmAveCharWidth, tm.tmPitchAndFamily);
    check(lstrcmp(face, "Arial") == 0 && tm.tmHeight - tm.tmInternalLeading >= 18 && tm.tmHeight - tm.tmInternalLeading <= 22
          && tm.tmAscent + tm.tmDescent == tm.tmHeight && (tm.tmPitchAndFamily & (TMPF_FIXED_PITCH | TMPF_TRUETYPE)) == (TMPF_FIXED_PITCH | TMPF_TRUETYPE)
          && (tm.tmPitchAndFamily & 0xf0) == FF_SWISS, line);
    wi = GetTextExtent(dc, "iiii", 4);
    ww = GetTextExtent(dc, "WWWW", 4);
    check(LOWORD(ww) > 2 * LOWORD(wi) && HIWORD(ww) == tm.tmHeight, "GetTextExtent: Arial is proportional, its height the cell's");
    GetCharWidth(dc, 'i', 'k', w);
    check(w[0] < w[2] && w[0] > 0, "GetCharWidth");
    fb = CreateFont(-20, 0, 0, 0, FW_BOLD, 0, 0, 0, ANSI_CHARSET, 0, 0, 0, VARIABLE_PITCH | FF_SWISS, "Arial");
    SelectObject(dc, fb);
    GetTextMetrics(dc, &tmb);
    check(tmb.tmWeight == FW_BOLD && LOWORD(GetTextExtent(dc, "WWWW", 4)) >= LOWORD(ww), "a bold font: its weight, no narrower");
    SelectObject(dc, old);
    DeleteObject(fb);
    DeleteObject(f);
    f = CreateFont(-16, 0, 0, 0, FW_NORMAL, 0, 0, 0, ANSI_CHARSET, 0, 0, 0, FIXED_PITCH | FF_MODERN, "Courier New");
    SelectObject(dc, f);
    GetTextMetrics(dc, &tm);
    check((tm.tmPitchAndFamily & TMPF_FIXED_PITCH) == 0 && tm.tmMaxCharWidth == tm.tmAveCharWidth, "Courier New: fixed pitch (TMPF_FIXED_PITCH clear)");
    SelectObject(dc, old);
    DeleteObject(f);
    f = CreateFont(0, 0, 0, 0, FW_NORMAL, 0, 0, 0, ANSI_CHARSET, 0, 0, 0, 0, "Helv");
    SelectObject(dc, f);
    GetTextFace(dc, sizeof face, face);
    check(lstrcmp(face, "MS Sans Serif") == 0, "Helv is MS Sans Serif");
    SelectObject(dc, old);
    DeleteObject(f);
    f = CreateFont(12, 0, 0, 0, FW_NORMAL, 0, 0, 0, OEM_CHARSET, 0, 0, 0, FIXED_PITCH, "Terminal");
    SelectObject(dc, f);
    GetTextMetrics(dc, &tm);
    check(tm.tmCharSet == OEM_CHARSET, "Terminal: OEM_CHARSET");
    SelectObject(dc, old);
    DeleteObject(f);
    SelectObject(dc, GetStockObject(SYSTEM_FONT));
    GetTextMetrics(dc, &tm);
    GetTextFace(dc, sizeof face, face);
    check(lstrcmp(face, "System") == 0 && tm.tmWeight == FW_BOLD && tm.tmHeight > 10, "the system font");
    ReleaseDC(hwnd, dc);
}

/* Windows DLLs: imports, usage counts, WEP. */
int FAR PASCAL W16ImpValue(void);
HINSTANCE FAR PASCAL W16ImpInstance(void);

static void dll2_tests(void)
{
    HINSTANCE d, d2, imp;
    char buf[80];
    int i;
    int (FAR PASCAL *f)(void);

    check(W16ImpValue() == 7, "a DLL the program imports: loaded with it, its LibMain ran first");
    imp = GetModuleHandle("W16IMP");
    check(imp != 0 && W16ImpInstance() == imp && GetModuleUsage(imp) >= 1, "GetModuleHandle of an imported DLL: its LibMain's instance");
    check(GetModuleFileName(imp, buf, sizeof buf) > 0 && strstr(buf, "W16IMP.DLL"), "GetModuleFileName of a DLL");
    d = LoadLibrary("W16IMP.DLL");
    check(d == imp && GetModuleUsage(imp) == 2, "LoadLibrary of an imported DLL: the same instance, one more user");
    FreeLibrary(d);
    check(GetModuleHandle("W16IMP") == imp && W16ImpValue() == 7, "FreeLibrary leaves an imported DLL loaded");
    f = (int (FAR PASCAL *)(void))GetProcAddress(imp, MAKEINTRESOURCE(1));
    check(f == NULL || f() == 7, "GetProcAddress by ordinal");
    f = (int (FAR PASCAL *)(void))GetProcAddress(imp, "W16IMPVALUE");
    check(f && f() == 7, "GetProcAddress by name");

    WritePrivateProfileString("run", "wep", "", "W16DLL.INI");
    d = LoadLibrary("W16DLL.DLL");
    d2 = LoadLibrary("W16DLL.DLL");
    wsprintf(line, "LoadLibrary twice: one instance, usage 2 (%04x %04x %d)", d, d2, GetModuleUsage(d));
    check(d > HINSTANCE_ERROR && d2 == d && GetModuleUsage(d) == 2, line);
    FreeLibrary(d);
    GetPrivateProfileString("run", "wep", "", buf, sizeof buf, "W16DLL.INI");
    check(GetModuleHandle("W16DLL") == d && buf[0] == 0, "the first FreeLibrary leaves it loaded");
    FreeLibrary(d);
    GetPrivateProfileString("run", "wep", "", buf, sizeof buf, "W16DLL.INI");
    check(GetModuleHandle("W16DLL") == 0 && lstrcmp(buf, "free") == 0, "the last FreeLibrary calls WEP(WEP_FREE_DLL) and unloads it");
    d = LoadLibrary("W16DLL.DLL");
    i = GetModuleUsage(d);
    FreeModule(d);
    wsprintf(line, "FreeModule (%04x, usage %d, then %04x)", d, i, GetModuleHandle("W16DLL"));
    check(GetModuleHandle("W16DLL") == 0, line);
    check(LoadLibrary("NOSUCH.DLL") < HINSTANCE_ERROR, "LoadLibrary of a missing DLL: an error below 32");
}

/* WIN.INI and private profiles: real files in the Windows directory. */
static int file_has(const char *path, const char *text)
{
    static char buf[2048];
    int fd = open(path, O_RDONLY | O_BINARY), n;

    if (fd < 0)
        return 0;
    n = read(fd, buf, sizeof buf - 1);
    close(fd);
    if (n < 0)
        return 0;
    buf[n] = 0;
    return strstr(buf, text) != NULL;
}

static void ini2_tests(void)
{
    char buf[80], dir[80];
    int fd;
    static char ini[] = "; a comment\r\n[First]\r\nKey = spaced value  \r\nNum=12abc\r\n\r\n[Second]\r\nA=1\r\n";

    check(GetWindowsDirectory(dir, sizeof dir) > 0 && lstrcmpi(dir, "C:\\WINDOWS") == 0, "GetWindowsDirectory");
    check(GetSystemDirectory(buf, sizeof buf) > 0 && lstrcmpi(buf, "C:\\WINDOWS\\SYSTEM") == 0, "GetSystemDirectory");
    WriteProfileString("W16Test2", "Colour", "blue");
    check(file_has("C:\\WINDOWS\\WIN.INI", "[W16Test2]") && file_has("C:\\WINDOWS\\WIN.INI", "Colour=blue"), "WriteProfileString writes C:\\WINDOWS\\WIN.INI");
    fd = open("C:\\WINDOWS\\W16T2.INI", O_CREAT | O_TRUNC | O_WRONLY | O_BINARY, 0666);
    write(fd, ini, sizeof ini - 1);
    close(fd);
    check(GetPrivateProfileString("first", "KEY", "", buf, sizeof buf, "W16T2.INI") == 12 && lstrcmp(buf, "spaced value") == 0,
          "GetPrivateProfileString: no case, the blanks around the value go");
    check(GetPrivateProfileInt("First", "Num", 0, "W16T2.INI") == 12, "GetPrivateProfileInt: the leading digits");
    check(GetPrivateProfileString("First", "Key", "", buf, 7, "W16T2.INI") == 6 && lstrcmp(buf, "spaced") == 0, "a value cut to the buffer");
    check(GetPrivateProfileString("First", "None", "default  ", buf, sizeof buf, "W16T2.INI") == 7 && lstrcmp(buf, "default") == 0,
          "the default without trailing blanks");
    WritePrivateProfileString("First", "New", "x", "W16T2.INI");
    check(file_has("C:\\WINDOWS\\W16T2.INI", "; a comment") && file_has("C:\\WINDOWS\\W16T2.INI", "Num=12abc\r\nNew=x"),
          "WritePrivateProfileString keeps comments and adds a key at its section's end");
    WritePrivateProfileString("Third", "B", "2", "C:\\W16T3.INI");
    check(file_has("C:\\W16T3.INI", "[Third]") && GetPrivateProfileInt("Third", "B", 0, "C:\\W16T3.INI") == 2, "a profile with a path is that file");
    check(WriteProfileString(NULL, NULL, NULL) || 1, "WriteProfileString(NULL, NULL, NULL) flushes");
    WriteProfileString("W16Test2", NULL, NULL);
    check(!file_has("C:\\WINDOWS\\WIN.INI", "[W16Test2]"), "WriteProfileString deletes its section");
    unlink("C:\\WINDOWS\\W16T2.INI");
    unlink("C:\\W16T3.INI");
}

/* W16TEST NAME...: only those groups of checks. */
static LPSTR only;

static int want(const char *name)
{
    char w[32];
    LPSTR p = only;
    int i;

    if (only == NULL || *only == 0)
        return 1;
    while (*p) {
        while (*p == ' ')
            p++;
        for (i = 0; *p && *p != ' ' && i < sizeof w - 1; )
            w[i++] = *p++;
        w[i] = 0;
        if (i && lstrcmpi(w, name) == 0)
            return 1;
    }
    return 0;
}

/* Stage 2's rest, each group alone with W16TEST GROUP. */
static void stage2b_tests(HINSTANCE hInst)
{
    HWND hwnd;

    if (want("MDI"))
        mdi2_tests(hInst);
    if (want("GDI"))
        gdi2_tests(hInst);
    if (want("RGN"))
        rgn2_tests(hInst);
    if (want("MAP"))
        map2_tests();
    if (want("FONT")) {
        WNDCLASS wc;
        memset(&wc, 0, sizeof wc);
        wc.lpfnWndProc = DefWindowProc;
        wc.hInstance = hInst;
        wc.lpszClassName = "W16Plain";
        RegisterClass(&wc);
        hwnd = CreateWindow("W16Plain", "W16TEST fonts", WS_OVERLAPPEDWINDOW, 20, 20, 200, 100, NULL, NULL, hInst, NULL);
        font2_tests(hwnd);
        DestroyWindow(hwnd);
    }
    if (want("DLL"))
        dll2_tests();
    if (want("INI"))
        ini2_tests();
}

int PASCAL WinMain(HINSTANCE hInst, HINSTANCE hPrev, LPSTR cmd, int show)
{
    out("w16test: rs2's native Win16 layer\r\n");
    only = cmd;
    if (!want("ALL")) {
        stage2b_tests(hInst);
        sprintf(line, "w16test done: %d failure(s)\r\n", failures);
        out(line);
        return failures;
    }
    kernel_tests();
    dos_tests();
    user_tests(hInst);
    stage2_tests(hInst);
    stage3_tests(hInst);
    real_app_tests(hInst);
    stage2b_tests(hInst);
    palette_tests(hInst);
    delphi_tests(hInst);
    toolhelp_tests(hInst);
    installer_tests(hInst);
    notebook_tests(hInst);
    nomove_tests(hInst);
    dll_tests();
    radpack_tests(hInst);
    dll_cycle_tests();
    sprintf(line, "w16test done: %d failure(s)\r\n", failures);
    out(line);
    return failures;
}
