// CXXTDL2.DLL: a DLL with a C++ interface, linked to by CXXTEST
// (docs/CXX64.md, "C++ DLLs").
#include "cxxtdl2.h"
#include <iostream>
namespace shapes {
namespace { struct Hidden {}; }
ShapeError::~ShapeError() = default;
int Shape::count = 0;
Shape::~Shape() = default;
std::string Shape::name() const { return "shape with a name longer than a short string"; }
Square::Square(double s) : side(s) {
    if (s < 0)
        throw ShapeError("negative side");
    count++;
}
double Square::area() const { return side * side; }
std::unique_ptr<Shape> make_square(double s) { return std::make_unique<Square>(s); }
double total(const std::vector<std::shared_ptr<Shape>> &v) {
    double t = 0;
    for (auto &s : v)
        t += s->area();
    return t;
}
const std::type_info &square_type() { return typeid(Square); }
const std::type_info &inline_error_type() { return typeid(InlineError); }
const std::type_info &hidden_type() { return typeid(Hidden); }
void call(const std::function<void()> &f, int *destroyed) {
    struct D { int *n; ~D() { ++*n; } } d{destroyed};
    f();
}
void fail_inline(int code) { throw InlineError(code); }
std::exception_ptr caught() {
    try { std::vector<int>().at(5); } catch (...) { return std::current_exception(); }
    return nullptr;
}
void print(const std::string &s) { std::cout << s; }
thread_local int calls = 10;
int calls_here() { return ++calls; }
int catch_from(const std::function<void()> &f) {
    try { f(); } catch (const InlineError &e) { return e.code; } catch (const std::exception &) { return -1; } catch (...) { return -2; }
    return 0;
}
}
