/* Calls functions assembled by wasm and one kept in a library made by wlib. */
#include <stdio.h>

extern int AsmMulAdd( int a, int b );
extern int AsmCountBits( unsigned v );
extern int LibTwice( int v );

int main( void )
{
    int r1 = AsmMulAdd( 6, 7 );
    int r2 = AsmCountBits( 0xF0F1 );
    int r3 = LibTwice( 21 );

    printf( "asmmain: AsmMulAdd %d, AsmCountBits %d, LibTwice %d\n", r1, r2, r3 );
    return( r1 != 49 || r2 != 9 || r3 != 42 );
}
