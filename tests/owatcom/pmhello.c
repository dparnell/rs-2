/* PM program: wcl386 -l=os2v2_pm pmhello.c; closes itself after 8 s */
#define INCL_WIN
#define INCL_GPI
#include <os2.h>
#include <stdio.h>

static MRESULT EXPENTRY ClientProc( HWND hwnd, ULONG msg, MPARAM mp1, MPARAM mp2 )
{
    HPS     hps;
    RECTL   rcl;

    switch( msg ) {
    case WM_CREATE:
        WinStartTimer( WinQueryAnchorBlock( hwnd ), hwnd, 1, 8000 );
        return( 0 );
    case WM_TIMER:
        WinPostMsg( hwnd, WM_QUIT, 0, 0 );
        return( 0 );
    case WM_PAINT:
        hps = WinBeginPaint( hwnd, NULLHANDLE, &rcl );
        WinQueryWindowRect( hwnd, &rcl );
        WinFillRect( hps, &rcl, CLR_PALEGRAY );
        WinDrawText( hps, -1, "Hello from Open Watcom PM on rs2", &rcl,
                     CLR_BLUE, CLR_PALEGRAY, DT_CENTER | DT_VCENTER );
        WinEndPaint( hps );
        return( 0 );
    }
    return( WinDefWindowProc( hwnd, msg, mp1, mp2 ) );
}

int main( void )
{
    HAB     hab;
    HMQ     hmq;
    HWND    frame, client;
    QMSG    qmsg;
    ULONG   fl = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST;
    FILE    *f;

    hab = WinInitialize( 0 );
    hmq = WinCreateMsgQueue( hab, 0 );
    WinRegisterClass( hab, "PmHello", ClientProc, CS_SIZEREDRAW, 0 );
    frame = WinCreateStdWindow( HWND_DESKTOP, WS_VISIBLE, &fl, "PmHello",
                                "Watcom PM Hello", 0, NULLHANDLE, 1, &client );
    WinSetWindowPos( frame, HWND_TOP, 200, 200, 480, 200, SWP_SIZE | SWP_MOVE | SWP_ACTIVATE | SWP_SHOW );
    while( WinGetMsg( hab, &qmsg, NULLHANDLE, 0, 0 ) )
        WinDispatchMsg( hab, &qmsg );
    WinDestroyWindow( frame );
    WinDestroyMsgQueue( hmq );
    WinTerminate( hab );
    f = fopen( "PMHELLO.OK", "w" );
    if( f != NULL ) {
        fprintf( f, "pmhello ran, frame %lx client %lx\n", frame, client );
        fclose( f );
    }
    return( 0 );
}
