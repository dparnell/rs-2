/* Open Watcom 2.0's C runtime on rs2: C99 (long long, <stdint.h>,
 * <inttypes.h>, snprintf, designated initialisers, _Bool,
 * alloca) and file I/O (stdio, POSIX io, directories, stat, time).
 *   wcl386 -za99 -l=os2v2 c99io.c        (and -bm -br, 16-bit -ml)
 * Ends with "c99io: N checks, M failures" and exit code M. */
#include <stdio.h>
#include <stdlib.h>
#include <malloc.h>
#include <string.h>
#include <stdint.h>
#include <inttypes.h>
#include <stdbool.h>
#include <time.h>
#include <errno.h>
#include <io.h>
#include <fcntl.h>
#include <direct.h>
#include <sys/types.h>
#include <sys/stat.h>

static int checks, fails;

static void check( const char *what, int ok )
{
    checks++;
    if( !ok ) {
        fails++;
        printf( "FAIL %s\n", what );
    }
}

struct pt { int x, y; const char *name; };

static long long fact( int n )
{
    return( n <= 1 ? 1 : n * fact( n - 1 ) );
}

static int alloca_sum( int n )
{
    int *a = alloca( n * sizeof( int ) );
    int i, s = 0;

    for( i = 0; i < n; i++ )
        a[i] = i * i;
    for( i = 0; i < n; i++ )
        s += a[i];
    return( s );
}

int main( void )
{
    char            buf[256];
    struct pt       p = { .y = 7, .name = "pt", .x = 3 };
    bool            b = true;
    int64_t         big = INT64_C( 1 ) << 40;
    uint32_t        u = UINT32_MAX;
    FILE            *f;
    int             fd, i, n;
    long            pos;
    struct stat     st;
    time_t          now;
    struct tm       *tm;
    DIR             *d;
    struct dirent   *de;

    /* C99 */
    snprintf( buf, sizeof( buf ), "%lld %" PRId64 " %" PRIu32, fact( 20 ), big, u );
    check( "long long and <inttypes.h> printf", strcmp( buf, "2432902008176640000 1099511627776 4294967295" ) == 0 );
    n = snprintf( buf, 4, "%s", "truncated" );
    check( "snprintf truncates and counts", n == 9 && strcmp( buf, "tru" ) == 0 );
    check( "designated initialisers", p.x == 3 && p.y == 7 && strcmp( p.name, "pt" ) == 0 );
    check( "_Bool", b == 1 && sizeof( b ) == 1 );
    check( "alloca", alloca_sum( 10 ) == 285 );
    check( "strtoll", strtoll( "-9223372036854775807", NULL, 10 ) == -INT64_MAX );
    check( "long long division", ( big / 3 ) == 366503875925LL && ( big % 7 ) == 2 );
    sprintf( buf, "%.3f %e %g", 3.14159, 12345.678, 0.0001 );
    check( "floating point printf", strcmp( buf, "3.142 1.234568e+04 0.0001" ) == 0 );

    /* stdio */
    f = fopen( "C99IO.TMP", "w+b" );
    check( "fopen w+b", f != NULL );
    if( f != NULL ) {
        for( i = 0; i < 1000; i++ )
            fprintf( f, "line %d\n", i );
        pos = ftell( f );
        check( "ftell after writes", pos == 8890 );
        fseek( f, 0, SEEK_SET );
        n = 0;
        while( fgets( buf, sizeof( buf ), f ) != NULL )
            n++;
        check( "fgets reads the lines back", n == 1000 );
        fseek( f, -9, SEEK_END );
        fgets( buf, sizeof( buf ), f );
        check( "fseek from the end", strcmp( buf, "line 999\n" ) == 0 );
        fclose( f );
    }
    /* POSIX-style I/O */
    fd = open( "C99IO.TMP", O_RDWR | O_BINARY );
    check( "open", fd >= 0 );
    if( fd >= 0 ) {
        check( "filelength", filelength( fd ) == 8890 );
        lseek( fd, 5, SEEK_SET );
        n = read( fd, buf, 4 );
        check( "lseek and read", n == 4 && memcmp( buf, "0\nli", 4 ) == 0 );
        lseek( fd, 0, SEEK_END );
        write( fd, "end\n", 4 );
        check( "chsize", chsize( fd, 100 ) == 0 && filelength( fd ) == 100 );
        close( fd );
    }
    check( "stat", stat( "C99IO.TMP", &st ) == 0 && st.st_size == 100 && S_ISREG( st.st_mode ) );
    remove( "C99IO.REN" );
    check( "rename", rename( "C99IO.TMP", "C99IO.REN" ) == 0 && access( "C99IO.REN", 0 ) == 0 );
    check( "remove", remove( "C99IO.REN" ) == 0 && access( "C99IO.REN", 0 ) != 0 && errno == ENOENT );
    /* directories */
    rmdir( "C99DIR" );
    check( "mkdir", mkdir( "C99DIR" ) == 0 );
    f = fopen( "C99DIR\\A.TXT", "w" );
    if( f != NULL ) {
        fputs( "a\n", f );
        fclose( f );
    }
    f = fopen( "C99DIR\\B.TXT", "w" );
    if( f != NULL )
        fclose( f );
    n = 0;
    d = opendir( "C99DIR\\*.TXT" );
    check( "opendir", d != NULL );
    if( d != NULL ) {
        while( (de = readdir( d )) != NULL ) {
            if( stricmp( de->d_name, "A.TXT" ) == 0 || stricmp( de->d_name, "B.TXT" ) == 0 ) {
                n++;
            }
        }
        closedir( d );
    }
    check( "readdir finds both files", n == 2 );
    remove( "C99DIR\\A.TXT" );
    remove( "C99DIR\\B.TXT" );
    check( "rmdir", rmdir( "C99DIR" ) == 0 );
    check( "getcwd", getcwd( buf, sizeof( buf ) ) != NULL && buf[1] == ':' );
    /* tmpfile, time */
    f = tmpfile();
    check( "tmpfile", f != NULL );
    if( f != NULL ) {
        fputs( "temp", f );
        rewind( f );
        check( "tmpfile reads back", fgets( buf, sizeof( buf ), f ) != NULL && strcmp( buf, "temp" ) == 0 );
        fclose( f );
    }
    now = time( NULL );
    tm = localtime( &now );
    check( "time and localtime", now > 1700000000 && tm != NULL && tm->tm_year >= 123 );
    strftime( buf, sizeof( buf ), "%Y-%m-%d", tm );
    check( "strftime", strlen( buf ) == 10 && buf[4] == '-' );
    check( "getenv", getenv( "WATCOM" ) != NULL );

    printf( "c99io: %d checks, %d failures\n", checks, fails );
    return( fails );
}
