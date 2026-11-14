/* Windows 3.x program built on rs2: wcl -l=windows -bt=windows winhello.c.
 * Opens a window, paints, writes WINHELLO.OUT and closes itself (timer). */
#include <windows.h>
#include <stdio.h>

static char szClass[] = "WatcomWinHello";

long FAR PASCAL WndProc( HWND hwnd, UINT msg, WPARAM wp, LPARAM lp )
{
    PAINTSTRUCT ps;
    HDC         hdc;
    RECT        rc;
    FILE        *f;

    switch( msg ) {
    case WM_CREATE:
        SetTimer( hwnd, 1, 3000, NULL );
        return( 0 );
    case WM_TIMER:
        KillTimer( hwnd, 1 );
        GetClientRect( hwnd, &rc );
        f = fopen( "WINHELLO.OUT", "w" );
        if( f != NULL ) {
            fprintf( f, "winhello: client %dx%d\n", rc.right, rc.bottom );
            fclose( f );
        }
        DestroyWindow( hwnd );
        return( 0 );
    case WM_PAINT:
        hdc = BeginPaint( hwnd, &ps );
        GetClientRect( hwnd, &rc );
        DrawText( hdc, "Hello from Open Watcom Win16 built on rs2", -1, &rc,
                  DT_CENTER | DT_VCENTER | DT_SINGLELINE );
        EndPaint( hwnd, &ps );
        return( 0 );
    case WM_DESTROY:
        PostQuitMessage( 0 );
        return( 0 );
    }
    return( DefWindowProc( hwnd, msg, wp, lp ) );
}

int PASCAL WinMain( HINSTANCE inst, HINSTANCE prev, LPSTR cmd, int show )
{
    WNDCLASS    wc;
    HWND        hwnd;
    MSG         msg;

    if( !prev ) {
        memset( &wc, 0, sizeof( wc ) );
        wc.lpfnWndProc = WndProc;
        wc.hInstance = inst;
        wc.hCursor = LoadCursor( NULL, IDC_ARROW );
        wc.hbrBackground = GetStockObject( WHITE_BRUSH );
        wc.lpszClassName = szClass;
        RegisterClass( &wc );
    }
    hwnd = CreateWindow( szClass, "Watcom Win16", WS_OVERLAPPEDWINDOW, 100, 100, 400, 150,
                         NULL, NULL, inst, NULL );
    ShowWindow( hwnd, show );
    UpdateWindow( hwnd );
    while( GetMessage( &msg, NULL, 0, 0 ) ) {
        TranslateMessage( &msg );
        DispatchMessage( &msg );
    }
    return( msg.wParam );
}
