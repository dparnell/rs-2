// MCPP: C++ in a MinGW-w64 program (static libstdc++): exceptions,
// constructors of statics, the STL.
#include <cstdio>
#include <string>
#include <vector>
#include <map>
#include <stdexcept>
#include <memory>
#include <algorithm>

static int fails = 0, checks = 0;
static void check(bool ok, const char *what)
{
    checks++;
    if (!ok) {
        fails++;
        std::printf("FAIL %s\n", what);
    }
}

struct Global {
    int v;
    Global() : v(42) {}
};
static Global g;

static int thrower(int x)
{
    if (x > 2)
        throw std::runtime_error("too big");
    return x;
}

int main()
{
    std::setvbuf(stdout, nullptr, _IONBF, 0);
    check(g.v == 42, "static constructor");
    std::string s = "hello";
    s += " world";
    check(s.size() == 11 && s.find("world") == 6, "std::string");
    std::vector<int> v{5, 1, 4};
    std::sort(v.begin(), v.end());
    check(v[0] == 1 && v[2] == 5, "std::vector, std::sort");
    std::map<std::string, int> m{{"a", 1}, {"b", 2}};
    check(m["b"] == 2 && m.size() == 2, "std::map");
    bool caught = false;
    try {
        thrower(5);
    } catch (const std::exception &e) {
        caught = std::string(e.what()) == "too big";
    }
    check(caught, "C++ exception caught");
    int n = 0;
    for (int i = 0; i < 5; i++) {
        try {
            n += thrower(i);
        } catch (...) {
            n += 100;
        }
    }
    check(n == 3 + 200, "exceptions in a loop");
    auto p = std::make_unique<std::string>("u");
    check(*p == "u", "unique_ptr");
    std::printf("mcpp done: %d failure(s) in %d checks\n", fails, checks);
    return fails;
}
