/* Opens NMPBIND's pipe (\PIPE\WDNMP.BND, made by the 16-bit
 * DosMakeNmPipe through NAMPIPES) as a client, as NMPSERV does. */
#include <stdio.h>
#define INCL_DOS
#include <os2.h>

int main( void )
{
    HFILE   h;
    ULONG   action, state = 0;
    APIRET  rc, rcw;

    rcw = DosWaitNPipe( "\\PIPE\\WDNMP.BND", 2000 );
    rc = DosOpen( "\\PIPE\\WDNMP.BND", &h, &action, 0, 0, OPEN_ACTION_OPEN_IF_EXISTS,
                  OPEN_ACCESS_READWRITE | OPEN_SHARE_DENYNONE, NULL );
    if( rc == 0 ) {
        DosQueryNPHState( h, &state );
        DosClose( h );
    }
    printf( "bindtest: DosWaitNPipe %lu, DosOpen %lu, state %lx\n", rcw, rc, state );
    return( rc != 0 );
}
