// C++ on rs2: wcl386 -l=os2v2 -xs -xr cpptest.cpp (also wcl -l=os2 for 16-bit)
// iostream, exceptions thrown through functions, templates, the standard
// containers and Watcom's own container classes, static constructors and
// destructors, RTTI.
#include <iostream>
#include <fstream>
#include <strstrea.h>
#include <string>
#include <vector>
#include <list>
#include <map>
#include <algorithm>
#include <stdexcept>
#include <typeinfo>
#include <wcvector.h>
#include <wclist.h>
#include <wclistit.h>
#include <stdio.h>

static int checks = 0, failures = 0;

static void check( bool ok, const char *what )
{
    ++checks;
    if( !ok ) {
        ++failures;
        std::cout << "FAIL: " << what << std::endl;
    }
}

// Static construction: runs before main; destruction after it.
struct Global {
    int value;
    Global() : value( 42 ) {}
    ~Global() {
        FILE *f = fopen( "CPPDTOR.OK", "w" );
        if( f ) {
            fprintf( f, "static destructor ran (value %d)\n", value );
            fclose( f );
        }
    }
};
static Global global;

template<class T> T biggest( const T *v, int n )
{
    T m = v[0];
    for( int i = 1; i < n; ++i )
        if( m < v[i] )
            m = v[i];
    return( m );
}

template<class T, int N> struct Stack {
    T items[N];
    int top;
    Stack() : top( 0 ) {}
    void push( const T &t ) {
        if( top == N )
            throw std::overflow_error( "stack full" );
        items[top++] = t;
    }
    T pop() {
        if( top == 0 )
            throw std::underflow_error( "stack empty" );
        return( items[--top] );
    }
};

struct MyError {
    int code;
    MyError( int c ) : code( c ) {}
};

struct Tracker {
    static int live;
    Tracker() { ++live; }
    ~Tracker() { --live; }
};
int Tracker::live = 0;

static void level3( int n )
{
    Tracker t;
    if( n > 2 )
        throw MyError( n );
}
static void level2( int n ) { Tracker t; level3( n + 1 ); }
static void level1( int n ) { Tracker t; level2( n + 1 ); }

struct Base { virtual ~Base() {} virtual int f() { return 1; } };
struct Derived : Base { int f() { return 2; } };

int main()
{
    std::cout << "cpptest: C++ on rs2" << std::endl;
    check( global.value == 42, "static constructor" );

    // iostream formatting
    // (Open Watcom 1.9's <sstream> is empty: its string streams are the
    // classic strstreams.)
    char sbuf[64];
    std::ostrstream os( sbuf, sizeof( sbuf ) );
    os << 12 << ' ' << std::hex << 255 << ' ' << std::dec << 3.5 << " end" << std::ends;
    check( std::string( sbuf ) == "12 ff 3.5 end", "ostrstream" );
    std::istrstream is( "7 8 nine" );
    int a, b;
    char w[16];
    is >> a >> b >> w;
    check( a == 7 && b == 8 && std::string( w ) == "nine", "istrstream" );

    // exceptions across functions, destructors run while unwinding
    int caught = 0;
    try {
        level1( 1 );
    } catch( MyError &e ) {
        caught = e.code;
    }
    check( caught == 3, "throw through three functions" );
    check( Tracker::live == 0, "destructors while unwinding" );
    try {
        Stack<int, 2> s;
        s.push( 1 ); s.push( 2 ); s.push( 3 );
        check( false, "overflow not thrown" );
    } catch( std::exception &e ) {
        check( std::string( e.what() ) == "stack full", "std::exception what()" );
    }
    try {
        throw 5;
    } catch( double ) {
        check( false, "int caught as double" );
    } catch( int i ) {
        check( i == 5, "catch int" );
    }
    try {
        try {
            throw std::string( "inner" );
        } catch( std::string & ) {
            throw;      // rethrow
        }
    } catch( std::string &s ) {
        check( s == "inner", "rethrow" );
    }

    // templates
    int iv[] = { 3, 9, 2 };
    double dv[] = { 1.5, -2.0, 0.25 };
    check( biggest( iv, 3 ) == 9 && biggest( dv, 3 ) == 1.5, "function templates" );

    // standard containers
    std::vector<int> v;
    for( int i = 10; i > 0; --i )
        v.push_back( i * i );
    std::sort( v.begin(), v.end() );
    check( v.size() == 10 && v[0] == 1 && v[9] == 100, "vector + sort" );
    std::list<std::string> l;
    l.push_back( "b" ); l.push_front( "a" ); l.push_back( "c" );
    std::string joined;
    for( std::list<std::string>::iterator it = l.begin(); it != l.end(); ++it )
        joined += *it;
    check( joined == "abc", "list" );
    std::map<std::string, int> m;
    m["one"] = 1; m["two"] = 2; m["three"] = 3;
    check( m.size() == 3 && m["two"] == 2 && m.find( "four" ) == m.end(), "map" );

    // Watcom container classes
    WCValOrderedVector<int> wv;
    wv.insert( 5 ); wv.insert( 6 ); wv.insert( 7 );
    check( wv.entries() == 3 && wv[1] == 6, "WCValOrderedVector" );
    WCValSList<int> wl;
    wl.append( 1 ); wl.append( 2 ); wl.insert( 0 );
    WCValSListIter<int> wit( wl );
    int sum = 0, n = 0;
    while( ++wit ) {
        sum += wit.current();
        ++n;
    }
    check( n == 3 && sum == 3 && wl.find( 0 ) == 0, "WCValSList" );

    // RTTI
    Base *p = new Derived;
    check( p->f() == 2 && dynamic_cast<Derived *>( p ) != 0, "virtual + dynamic_cast" );
    check( typeid( *p ) == typeid( Derived ), "typeid" );
    delete p;

    // file streams
    {
        std::ofstream out( "CPPTEST.TXT" );
        out << "line one\n" << 1234 << "\n";
    }
    std::ifstream in( "CPPTEST.TXT" );
    char line[32];
    int num = 0;
    in.getline( line, sizeof( line ) );
    in >> num;
    check( std::string( line ) == "line one" && num == 1234, "fstream" );

    std::cout << "cpptest: " << checks << " checks, " << failures << " failures" << std::endl;
    return( failures != 0 );
}
