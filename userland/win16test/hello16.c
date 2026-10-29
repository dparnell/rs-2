/* HELLO16: a classic Windows 3.x hello world, the first test of rs2's
 * native Win16 layer (docs/WIN16.md). A window class, a WM_PAINT that
 * draws text, a MessageBox on a click. `HELLO16 GPF` writes to its own
 * code segment first: a General Protection Fault (PMXTEST checks WIN16's
 * "Application Error" for it). Built with Open Watcom's Win16 target
 * (build.sh). */

#include <windows.h>

static char szClass[] = "Hello16";

LONG FAR PASCAL _export HelloWndProc(HWND hwnd, UINT msg, WPARAM wp, LPARAM lp)
{
    PAINTSTRUCT ps;
    RECT rc;
    HDC hdc;

    switch (msg) {
    case WM_PAINT:
        hdc = BeginPaint(hwnd, &ps);
        GetClientRect(hwnd, &rc);
        TextOut(hdc, 10, 10, "Hello from Win16!", 17);
        DrawText(hdc, "Click to see a message box.", -1, &rc,
                 DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        EndPaint(hwnd, &ps);
        return 0;
    case WM_LBUTTONDOWN:
        MessageBox(hwnd, "You clicked the Win16 window.", "Hello16",
                   MB_OK | MB_ICONINFORMATION);
        return 0;
    case WM_DESTROY:
        PostQuitMessage(0);
        return 0;
    }
    return DefWindowProc(hwnd, msg, wp, lp);
}

int PASCAL WinMain(HINSTANCE hInst, HINSTANCE hPrev, LPSTR cmd, int show)
{
    WNDCLASS wc;
    HWND hwnd;
    MSG msg;

    if (cmd[0] == 'G' && cmd[1] == 'P' && cmd[2] == 'F')
        *(char FAR *)HelloWndProc = 0;
    if (!hPrev) {
        wc.style = CS_HREDRAW | CS_VREDRAW;
        wc.lpfnWndProc = HelloWndProc;
        wc.cbClsExtra = 0;
        wc.cbWndExtra = 0;
        wc.hInstance = hInst;
        wc.hIcon = LoadIcon(NULL, IDI_APPLICATION);
        wc.hCursor = LoadCursor(NULL, IDC_ARROW);
        wc.hbrBackground = GetStockObject(WHITE_BRUSH);
        wc.lpszMenuName = NULL;
        wc.lpszClassName = szClass;
        if (!RegisterClass(&wc))
            return 0;
    }
    hwnd = CreateWindow(szClass, "Hello Win16", WS_OVERLAPPEDWINDOW,
                        CW_USEDEFAULT, CW_USEDEFAULT, 360, 200,
                        NULL, NULL, hInst, NULL);
    ShowWindow(hwnd, show);
    UpdateWindow(hwnd);
    while (GetMessage(&msg, NULL, 0, 0)) {
        TranslateMessage(&msg);
        DispatchMessage(&msg);
    }
    return msg.wParam;
}
