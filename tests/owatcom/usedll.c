/* Uses mydll.dll through its import library and through DosLoadModule
 * (Watcom's register convention exports DllAdd as DllAdd_). */
#include <stdio.h>
#include <string.h>
#define INCL_DOS
#include <os2.h>

int DllAdd( int a, int b );
void DllFormat( char *buf, const char *who );

int main( void )
{
    char        buf[80], err[64];
    HMODULE     hmod;
    PFN         pfn;
    APIRET      rc;
    int         r1, r2 = -1;

    r1 = DllAdd( 2, 3 );
    DllFormat( buf, "usedll" );
    rc = DosLoadModule( err, sizeof( err ), "MYDLL", &hmod );
    if( rc == 0 && DosQueryProcAddr( hmod, 0, "DllAdd_", &pfn ) == 0 ) {
        r2 = ((int (*)( int, int ))pfn)( 10, 20 );
    }
    printf( "usedll: DllAdd %d, %s, DosLoadModule rc %lu, by name %d\n", r1, buf, rc, r2 );
    return( r1 != 5 || r2 != 30 || strcmp( buf, "hello usedll from mydll (6)" ) != 0 );
}
