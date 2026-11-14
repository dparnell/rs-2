/* Multithreaded C: wcl386 -l=os2v2 -bm mttest.c (and -br for the runtime DLL)
 * _beginthread, per-thread errno and strtok, a mutex, DosWaitThread. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <errno.h>
#include <process.h>
#include <stddef.h>
#define INCL_DOS
#include <os2.h>

#define THREADS 4
#define LOOPS   2000

static HMTX     mtx;
static long     counter;
static int      ok[THREADS];
static TID      tids[THREADS];

static void worker( void *arg )
{
    int     n = (int)arg;
    int     i;
    char    buf[32];
    char    *tok;

    errno = 100 + n;            /* per-thread errno */
    sprintf( buf, "t%d a b c", n );
    tok = strtok( buf, " " );   /* per-thread strtok state */
    for( i = 0; i < LOOPS; i++ ) {
        DosRequestMutexSem( mtx, SEM_INDEFINITE_WAIT );
        counter++;
        DosReleaseMutexSem( mtx );
        if( (i % 500) == 0 ) {
            DosSleep( 1 );
        }
    }
    tok = strtok( NULL, " " );
    ok[n] = ( errno == 100 + n && tok != NULL && strcmp( tok, "a" ) == 0 && *_threadid != 0 );
}

int main( void )
{
    int     i, good = 0;
    char    *mem;

    DosCreateMutexSem( NULL, &mtx, 0, FALSE );
    for( i = 0; i < THREADS; i++ ) {
        tids[i] = _beginthread( worker, NULL, 16384, (void *)i );
        if( tids[i] == (TID)-1 ) {
            printf( "_beginthread failed\n" );
            return( 1 );
        }
    }
    mem = malloc( 100000 );     /* the heap while threads run */
    for( i = 0; i < THREADS; i++ ) {
        TID t = tids[i];
        DosWaitThread( &t, DCWW_WAIT );
    }
    free( mem );
    for( i = 0; i < THREADS; i++ ) {
        good += ok[i];
    }
    printf( "mttest: counter %ld (expected %d), %d of %d threads ok\n",
            counter, THREADS * LOOPS, good, THREADS );
    return( counter != THREADS * LOOPS || good != THREADS );
}
