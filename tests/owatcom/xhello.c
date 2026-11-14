/* Plain C for the cross targets: wcl -l=dos, wcl386 -l=dos4g, wcl386 -l=nt.
 * Writes XHELLO.OUT (the DOS session's console is not the build log). */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main( int argc, char **argv )
{
    FILE    *f;
    char    *p;
    long    sum = 0;
    int     i;

    p = malloc( 30000 );
    for( i = 0; i < 30000; i++ )
        p[i] = (char)i;
    for( i = 0; i < 30000; i++ )
        sum += (unsigned char)p[i];
    free( p );
    printf( "xhello: %d-bit, sum %ld, argv[1] %s\n", (int)sizeof( int ) * 8, sum,
            argc > 1 ? argv[1] : "-" );
    f = fopen( argc > 2 ? argv[2] : "XHELLO.OUT", "w" );
    if( f != NULL ) {
        fprintf( f, "xhello: %d-bit, sum %ld, %.3f\n", (int)sizeof( int ) * 8, sum, 2.0 / 3.0 );
        fclose( f );
    }
    return( sum != 3820008L );
}
