/* 16-bit OS/2 program: wcl -l=os2 hello16.c */
#include <stdio.h>
#define INCL_DOS
#include <os2.h>

int main( void )
{
    USHORT  ver[2];

    DosGetVersion( ver );
    printf( "Hello from a 16-bit OS/2 program, version %u.%u, far data %d\n",
            ver[0] >> 8, ver[0] & 0xff, (int)sizeof( void far * ) );
    return( 0 );
}
