// A C++ DLL on the C/C++ runtime DLLs: wcl386 -l=os2v2_dll -bd -br -xs cppdll.cpp
#include <string>
#include <stdexcept>
#include <iostream>
#include <stdio.h>

class __declspec(dllexport) Counter {
public:
    Counter( const char *name );
    ~Counter();
    int bump( int by );
    std::string describe() const;
private:
    std::string name_;
    int         count_;
};

static int instances;

Counter::Counter( const char *name ) : name_( name ), count_( 0 ) { ++instances; }
Counter::~Counter() { --instances; }

int Counter::bump( int by )
{
    if( by < 0 )
        throw std::invalid_argument( "negative bump" );
    count_ += by;
    return( count_ );
}

std::string Counter::describe() const
{
    char buf[16];
    sprintf( buf, "%d", count_ );
    return( name_ + "=" + buf );
}

extern "C" int __export CounterInstances( void ) { return( instances ); }
