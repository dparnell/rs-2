/* The C half of FMIX.FOR: Fortran names are upper case without the
 * trailing underscore of C's register convention ("^"), and Fortran
 * passes every argument by reference. */
#include <stdio.h>

#pragma aux CADD "^"
#pragma aux CSUMV "^"
#pragma aux CHALF "^"
#pragma aux CEACH "^"
#pragma aux CPRINT "^"
#pragma aux FSCALE "^"

extern void FSCALE( double *x );

long CADD( long *a, long *b )
{
    return( *a + *b );
}

long CSUMV( long *v, long *n )
{
    long    i, s = 0;

    for( i = 0; i < *n; i++ )
        s += v[i];
    return( s );
}

void CHALF( double *d )
{
    *d /= 2.0;
}

void CEACH( double *v, long *n )
{
    long    i;

    for( i = 0; i < *n; i++ )
        FSCALE( &v[i] );
}

void CPRINT( double *d )
{
    printf( "C printf of a Fortran value: %.4f\n", *d );
    fflush( stdout );
}
