/* W16TEST: checks of rs2's native Win16 layer (docs/WIN16.md). A
 * Windows 3.x program that calls KERNEL, USER and GDI and the DOS API
 * and writes PASS/FAIL lines to standard output (INT 21h), ending with
 * "w16test done: N failure(s)". Run it with
 * cargo xtask run --release --headless --timeout 150 --init "RS2\W16TEST.EXE"
 */

#include <windows.h>
#include <string.h>
#include <stdio.h>
#include <io.h>
#include <fcntl.h>
#include "w16test.h"

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

int PASCAL WinMain(HINSTANCE hInst, HINSTANCE hPrev, LPSTR cmd, int show)
{
    out("w16test: rs2's native Win16 layer\r\n");
    kernel_tests();
    dos_tests();
    user_tests(hInst);
    stage2_tests(hInst);
    sprintf(line, "w16test done: %d failure(s)\r\n", failures);
    out(line);
    return failures;
}
