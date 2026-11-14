/* waitfile FILE SECONDS: waits until FILE exists (programs started in other
 * sessions write their results so), then types it. */
#include <stdio.h>
#include <stdlib.h>
#define INCL_DOS
#include <os2.h>

int main( int argc, char **argv )
{
    FILE    *f = NULL;
    int     n, c;

    if( argc < 3 )
        return( 2 );
    for( n = atoi( argv[2] ) * 4; n > 0; n-- ) {
        f = fopen( argv[1], "r" );
        if( f != NULL )
            break;
        DosSleep( 250 );
    }
    if( f == NULL ) {
        printf( "waitfile: %s did not appear\n", argv[1] );
        return( 1 );
    }
    DosSleep( 500 );
    while( (c = fgetc( f )) != EOF )
        putchar( c );
    fclose( f );
    return( 0 );
}
