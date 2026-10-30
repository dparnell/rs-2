/* Text-mode OS/2 program: wcl386 -l=os2v2 hello.c */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#define INCL_DOS
#include <os2.h>

int main( int argc, char **argv )
{
    PTIB    ptib;
    PPIB    ppib;
    char    buf[64];
    int     i;

    DosGetInfoBlocks( &ptib, &ppib );
    printf( "Hello from Open Watcom C on rs2 (pid %lu)\n", ppib->pib_ulpid );
    for( i = 0; i < argc; i++ ) {
        printf( "argv[%d] = %s\n", i, argv[i] );
    }
    sprintf( buf, "%5.2f %s", 3.14159, getenv( "WATCOM" ) );
    printf( "sprintf: %s, strlen %u\n", buf, (unsigned)strlen( buf ) );
    return( 0 );
}
