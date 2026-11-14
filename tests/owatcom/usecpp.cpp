// Uses cppdll.dll's class; an exception thrown in the DLL is caught here.
// wcl386 -l=os2v2 -br -xs usecpp.cpp cppdll.lib
#include <string>
#include <stdexcept>
#include <iostream>

class __declspec(dllimport) Counter {
public:
    Counter( const char *name );
    ~Counter();
    int bump( int by );
    std::string describe() const;
private:
    std::string name_;
    int         count_;
};
extern "C" int CounterInstances( void );

int main()
{
    int bad = 0;
    std::string d;
    {
        Counter c( "apples" );
        c.bump( 2 );
        c.bump( 3 );
        d = c.describe();
        if( d != "apples=5" || CounterInstances() != 1 )
            bad |= 1;
        try {
            c.bump( -1 );
            bad |= 2;
        } catch( std::invalid_argument &e ) {
            if( std::string( e.what() ) != "negative bump" )
                bad |= 4;
        }
    }
    if( CounterInstances() != 0 )
        bad |= 8;
    std::cout << "usecpp: " << d.c_str() << ", errors " << bad << std::endl;
    return( bad );
}
