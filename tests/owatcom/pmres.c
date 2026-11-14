/* A PM program whose menu, icon, string table and dialog come from an .rc
 * compiled by wrc: wcl386 -l=os2v2_pm pmres.c, wrc pmres.rc pmres.exe.
 * It checks what it loads, shows the dialog (dismissed by a timer) and
 * writes PMRES.OK. */
#define INCL_WIN
#define INCL_GPI
#include <os2.h>
#include <stdio.h>
#include <string.h>
#include "pmres.h"

static FILE *log;
static int  bad;

static void check( int ok, const char *what )
{
    fprintf( log, "%s: %s\n", ok ? "ok" : "FAIL", what );
    if( !ok )
        bad++;
}

static MRESULT EXPENTRY DlgProc( HWND hwnd, ULONG msg, MPARAM mp1, MPARAM mp2 )
{
    char    buf[64];

    switch( msg ) {
    case WM_INITDLG:
        WinQueryDlgItemText( hwnd, IDC_TEXT, sizeof( buf ), buf );
        check( strcmp( buf, "Text from the dialog template" ) == 0, "dialog static text" );
        WinQueryDlgItemText( hwnd, IDC_ENTRY, sizeof( buf ), buf );
        check( strcmp( buf, "entry" ) == 0, "dialog entry field" );
        WinStartTimer( WinQueryAnchorBlock( hwnd ), hwnd, 7, 1500 );
        return( 0 );
    case WM_TIMER:
        WinStopTimer( WinQueryAnchorBlock( hwnd ), hwnd, 7 );
        WinDismissDlg( hwnd, 1234 );
        return( 0 );
    }
    return( WinDefDlgProc( hwnd, msg, mp1, mp2 ) );
}

static MRESULT EXPENTRY ClientProc( HWND hwnd, ULONG msg, MPARAM mp1, MPARAM mp2 )
{
    HPS     hps;
    RECTL   rcl;
    char    buf[64];

    switch( msg ) {
    case WM_CREATE:
        WinPostMsg( hwnd, WM_COMMAND, MPFROMSHORT( IDM_HELLO ), 0 );
        return( 0 );
    case WM_COMMAND:
        if( SHORT1FROMMP( mp1 ) == IDM_HELLO ) {
            ULONG r = WinDlgBox( HWND_DESKTOP, hwnd, DlgProc, NULLHANDLE, ID_DLG, NULL );
            check( r == 1234, "WinDlgBox from the template" );
            WinPostMsg( hwnd, WM_COMMAND, MPFROMSHORT( IDM_EXIT ), 0 );
        } else if( SHORT1FROMMP( mp1 ) == IDM_EXIT ) {
            WinPostMsg( hwnd, WM_QUIT, 0, 0 );
        }
        return( 0 );
    case WM_PAINT:
        hps = WinBeginPaint( hwnd, NULLHANDLE, &rcl );
        WinQueryWindowRect( hwnd, &rcl );
        WinFillRect( hps, &rcl, CLR_PALEGRAY );
        WinLoadString( WinQueryAnchorBlock( hwnd ), NULLHANDLE, IDS_HELLO, sizeof( buf ), buf );
        WinDrawText( hps, -1, buf, &rcl, CLR_BLUE, CLR_PALEGRAY, DT_CENTER | DT_VCENTER );
        WinEndPaint( hps );
        return( 0 );
    }
    return( WinDefWindowProc( hwnd, msg, mp1, mp2 ) );
}

int main( void )
{
    HAB     hab;
    HMQ     hmq;
    HWND    frame, client, menu;
    QMSG    qmsg;
    ULONG   fl = FCF_TITLEBAR | FCF_SYSMENU | FCF_SIZEBORDER | FCF_MINMAX | FCF_TASKLIST |
                 FCF_MENU | FCF_ICON;
    char    buf[64];
    LONG    n;
    HPOINTER icon;

    log = fopen( "PMRES.OK", "w" );
    hab = WinInitialize( 0 );
    hmq = WinCreateMsgQueue( hab, 0 );
    n = WinLoadString( hab, NULLHANDLE, IDS_SECOND, sizeof( buf ), buf );
    check( n == 13 && strcmp( buf, "Second string" ) == 0, "WinLoadString" );
    icon = WinLoadPointer( HWND_DESKTOP, NULLHANDLE, ID_MAIN );
    check( icon != NULLHANDLE, "WinLoadPointer (the icon)" );
    WinRegisterClass( hab, "PmRes", ClientProc, CS_SIZEREDRAW, 0 );
    frame = WinCreateStdWindow( HWND_DESKTOP, 0, &fl, "PmRes",
                                "Watcom resources", 0, NULLHANDLE, ID_MAIN, &client );
    check( frame != NULLHANDLE, "WinCreateStdWindow with FCF_MENU | FCF_ICON" );
    menu = WinWindowFromID( frame, FID_MENU );
    check( menu != NULLHANDLE, "frame menu" );
    n = (LONG)WinSendMsg( menu, MM_QUERYITEMTEXT, MPFROM2SHORT( IDM_EXIT, sizeof( buf ) ), MPFROMP( buf ) );
    check( n == 5 && strcmp( buf, "E~xit" ) == 0, "menu item text (in the File submenu)" );
    check( WinSendMsg( frame, WM_QUERYICON, 0, 0 ) != 0, "frame icon" );
    WinSetWindowPos( frame, HWND_TOP, 150, 150, 420, 200, SWP_SIZE | SWP_MOVE | SWP_ACTIVATE | SWP_SHOW );
    while( WinGetMsg( hab, &qmsg, NULLHANDLE, 0, 0 ) )
        WinDispatchMsg( hab, &qmsg );
    WinDestroyWindow( frame );
    WinDestroyMsgQueue( hmq );
    WinTerminate( hab );
    fprintf( log, "pmres: %d failures\n", bad );
    fclose( log );
    return( bad );
}
