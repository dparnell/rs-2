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
    n = pm_acks;
    PostMessage(pm_server, WM_DDE_POKE, (WPARAM)hwnd, MAKELONG(h, GlobalAddAtom("Value")));
    pump(&pm_acks, n);
    check(pm_acks == n + 1 && (pm_status & 0x8000), "WM_DDE_POKE to the PM server: a positive WM_DDE_ACK");
    h = GlobalAlloc(GMEM_MOVEABLE | GMEM_DDESHARE, 7);
    lstrcpy(GlobalLock(h), "[quit]");
    GlobalUnlock(h);
    n = pm_acks;
    PostMessage(pm_server, WM_DDE_EXECUTE, (WPARAM)hwnd, MAKELONG(0, h));
    pump(&pm_acks, n);
    wsprintf(line, "WM_DDE_EXECUTE to the PM server: ACK with the commands back (%d %04x %04x %04x)", pm_acks - n, pm_status, pm_hi, h);
    check(pm_acks == n + 1 && (pm_status & 0x8000) && pm_hi == h, line);
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

/* The pixel at (x, y) of a window, once it has painted. */
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
    check(c == GetSysColor(COLOR_BTNFACE), buf);
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
    check(c == GetSysColor(COLOR_BTNFACE), buf);
    c = pixel_at(parent, 155, 45);
    wsprintf(buf, "a button inside an earlier group box shows, is %06lX", c);
    check(c == GetSysColor(COLOR_BTNFACE), buf);
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

    /* What Windows Setup writes: WIN.INI's [intl], SYSTEM.INI's code
     * page; SHARE is loaded (INT 2Fh AX=1000h). */
    check(GetProfileInt("intl", "iCountry", 0) != 0, "WIN.INI [intl] iCountry from the country settings");
    GetProfileString("intl", "sLanguage", "", buf, sizeof buf);
    check(lstrlen(buf) == 3, "WIN.INI [intl] sLanguage");
    check(GetPrivateProfileInt("boot.description", "codepage", 0, "SYSTEM.INI") != 0, "SYSTEM.INI [boot.description] codepage");
    check(share_state() == 0xff, "INT 2Fh AX=1000h: SHARE is loaded");

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

int PASCAL WinMain(HINSTANCE hInst, HINSTANCE hPrev, LPSTR cmd, int show)
{
    out("w16test: rs2's native Win16 layer\r\n");
    kernel_tests();
    dos_tests();
    user_tests(hInst);
    stage2_tests(hInst);
    stage3_tests(hInst);
    real_app_tests(hInst);
    delphi_tests(hInst);
    toolhelp_tests(hInst);
    installer_tests(hInst);
    sprintf(line, "w16test done: %d failure(s)\r\n", failures);
    out(line);
    return failures;
}
