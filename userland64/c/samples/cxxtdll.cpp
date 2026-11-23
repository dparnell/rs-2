// CXXTDLL.DLL: a C++ DLL for CXXTEST (docs/CXX64.md). It has its own copy
// of the C++ library, so exceptions stay inside and the interface is C.
#include <os2.h>
#include <cstdio>
#include <map>
#include <stdexcept>
#include <string>

namespace {
struct Registry {
    std::map<std::string, int> m{{"one", 1}, {"two", 2}};
    ~Registry() {
        if (FILE *f = std::fopen("CXXTDLL.FIN", "w")) {
            std::fputs("destroyed", f);
            std::fclose(f);
        }
    }
} registry;   // constructed when the DLL is loaded
thread_local int calls;
}

extern "C" RS2_EXPORT int dll_lookup(const char *name) {
    calls++;
    try {
        auto it = registry.m.find(name);
        if (it == registry.m.end())
            throw std::out_of_range(std::string("no ") + name);
        return it->second;
    } catch (const std::out_of_range &e) {
        return e.what()[0] == 'n' ? -1 : -2;
    }
}

extern "C" RS2_EXPORT int dll_calls() { return calls; }
