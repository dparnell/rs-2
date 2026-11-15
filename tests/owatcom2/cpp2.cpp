// Open Watcom 2.0's C++ library on rs2: std::string, the standard
// exceptions thrown by the library (vector::at, string::substr, new),
// containers and algorithms, iostream formatting, exceptions across
// threads (each thread its own exception state).
//   wcl386 -xs -xr -l=os2v2 -bm cpp2.cpp  (and -br; 16-bit: wcl -xs -xr -ml -l=os2)
// Ends with "cpp2: N checks, M failures" and exit code M.
#include <iostream>
#include <iomanip>
#include <fstream>
#include <strstrea>
#include <string>
#include <vector>
#include <list>
#include <deque>
#include <map>
#include <set>
#include <algorithm>
#include <numeric>
#include <stdexcept>
#include <memory>
#include <typeinfo>
#include <cstring>
#include <cstdio>
#include <process.h>
#define INCL_DOS
#include <os2.h>

static int checks, fails;

static void check( const char *what, bool ok )
{
    checks++;
    if( !ok ) {
        fails++;
        std::cout << "FAIL " << what << std::endl;
    }
}

struct Shape {
    virtual ~Shape() {}
    virtual double area() const = 0;
};
struct Square : Shape {
    double s;
    Square( double x ) : s( x ) {}
    double area() const { return s * s; }
};

class MyError : public std::runtime_error {
public:
    int code;
    MyError( const std::string &m, int c ) : std::runtime_error( m ), code( c ) {}
};

static int thrower( int depth )
{
    std::string guard( "on the stack while unwinding" );
    if( depth == 0 )
        throw MyError( "deep", 42 );
    return thrower( depth - 1 ) + 1;
}

#if !defined( _M_I86 )
static volatile int thread_ok[4];

static void worker( void *arg )
{
    int n = (int)arg;
    int caught = 0;
    for( int i = 0; i < 50; i++ ) {
        try {
            thrower( n + i % 5 );
        } catch( MyError &e ) {
            if( e.code == 42 )
                caught++;
        }
        DosSleep( 0 );
    }
    thread_ok[n] = ( caught == 50 ) ? 1 : 2;
}
#endif

int main()
{
    // std::string
    std::string s( "Hello" );
    s += ", rs2";
    s.insert( 5, " there" );
    check( "string append and insert", s == "Hello there, rs2" );
    check( "string find and substr", s.find( "rs2" ) == 13 && s.substr( 6, 5 ) == "there" );
    check( "string compare", std::string( "abc" ) < std::string( "abd" ) );
    // the library's exceptions
    std::vector< int > v;
    for( int i = 0; i < 10; i++ )
        v.push_back( 10 - i );
    bool caught = false;
    try {
        v.at( 20 ) = 1;
    } catch( std::out_of_range & ) {
        caught = true;
    }
    check( "vector::at throws std::out_of_range", caught );
    caught = false;
    try {
        s.substr( 100 );
    } catch( std::exception &e ) {
        caught = typeid( e ) == typeid( std::out_of_range );
    }
    check( "string::substr throws std::out_of_range (typeid)", caught );
    try {
        thrower( 5 );
        check( "throw through 6 frames", false );
    } catch( std::runtime_error &e ) {
        check( "throw through 6 frames, caught as the base class", std::strcmp( e.what(), "deep" ) == 0 );
    }
    // containers and algorithms
    std::sort( v.begin(), v.end() );
    check( "sort", v.front() == 1 && v.back() == 10 && std::accumulate( v.begin(), v.end(), 0 ) == 55 );
    check( "binary_search", std::binary_search( v.begin(), v.end(), 7 ) );
    std::map< std::string, int > m;
    m["one"] = 1;
    m["two"] = 2;
    m["three"] = 3;
    check( "map", m.size() == 3 && m["two"] == 2 && m.find( "four" ) == m.end() );
    std::set< int > st;
    for( unsigned k = 0; k < v.size(); k++ )
        st.insert( v[k] );
    st.insert( 5 );
    check( "set", st.size() == 10 && *st.begin() == 1 );
    std::list< std::string > l;
    l.push_back( "b" );
    l.push_front( "a" );
    l.push_back( "c" );
    l.reverse();
    check( "list", l.front() == "c" && l.size() == 3 );
    std::deque< int > dq;
    dq.push_front( 1 );
    dq.push_back( 2 );
    check( "deque", dq[0] == 1 && dq[1] == 2 );
    // polymorphism, RTTI
    std::auto_ptr< Shape > sh( new Square( 3 ) );
    check( "virtual call", sh->area() == 9.0 );
    check( "dynamic_cast", dynamic_cast< Square * >( sh.get() ) != 0 );
    // iostream formatting into a string stream
    char buf[100];
    std::ostrstream os( buf, sizeof( buf ) );
    os << std::setw( 5 ) << 42 << '|' << std::hex << 255 << '|' << std::setprecision( 3 ) << 3.14159 << std::ends;
    check( "iostream formatting", std::strcmp( buf, "   42|ff|3.14" ) == 0 );
    // fstream
    {
        std::ofstream out( "CPP2.TMP" );
        out << "first " << 123 << std::endl;
    }
    {
        std::ifstream in( "CPP2.TMP" );
        char w[32];
        int n = 0;
        in >> w >> n;
        check( "fstream write and read", std::strcmp( w, "first" ) == 0 && n == 123 );
    }
    std::remove( "CPP2.TMP" );
#if !defined( _M_I86 )
    // exceptions in four threads at once
    int tids[4];
    for( int i = 0; i < 4; i++ )
        tids[i] = _beginthread( worker, NULL, 32768, (void *)i );
    for( int i = 0; i < 4; i++ ) {
        TID t = tids[i];
        DosWaitThread( &t, DCWW_WAIT );
    }
    check( "exceptions in four threads", thread_ok[0] == 1 && thread_ok[1] == 1 && thread_ok[2] == 1 && thread_ok[3] == 1 );
#endif
    std::cout << "cpp2: " << checks << " checks, " << fails << " failures" << std::endl;
    return fails;
}
