/* A DLL: wcl386 -l=os2v2_dll -bd mydll.c (exports by __export / the .lnk) */
#include <stdio.h>
#include <string.h>

static int  loads;

/* Called by the C runtime when the DLL is loaded and freed. */
unsigned __dll_initialize( void )
{
    loads++;
    return( 1 );
}

unsigned __dll_terminate( void )
{
    return( 1 );
}

int __export DllAdd( int a, int b )
{
    return( a + b + loads - 1 );
}

void __export DllFormat( char *buf, const char *who )
{
    sprintf( buf, "hello %s from mydll (%u)", who, (unsigned)strlen( who ) );
}
