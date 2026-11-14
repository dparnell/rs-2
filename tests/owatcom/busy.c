/* Something for wsample to sample: wcl386 -l=os2v2 -d2 busy.c,
 * wsample busy.exe (writes BUSY.SMP), wprof busy.smp. */
#include <stdio.h>
#include <time.h>

static double inner( int n )
{
    double  s = 0;
    int     i;

    for( i = 1; i < n; i++ )
        s += 1.0 / ( (double)i * i );
    return( s );
}

static double outer( void )
{
    double  t = 0;
    int     k;

    for( k = 0; k < 40; k++ )
        t += inner( 20000 + k );
    return( t );
}

int main( void )
{
    clock_t start = clock();
    double  t = 0;
    int     rounds = 0;

    while( clock() - start < 3 * CLOCKS_PER_SEC ) {
        t += outer();
        rounds++;
    }
    printf( "busy: %d rounds, %.6f\n", rounds, t / rounds );
    return( 0 );
}
