// The C++ interface of CXXTDL2.DLL (docs/CXX64.md, "C++ DLLs").
#ifndef CXXTDL2_H
#define CXXTDL2_H
#include <os2.h>
#include <exception>
#include <functional>
#include <memory>
#include <stdexcept>
#include <string>
#include <typeinfo>
#include <vector>
namespace shapes {
// Described in each module that uses it.
struct InlineError : std::runtime_error {
    int code;
    explicit InlineError(int c) : std::runtime_error("inline error"), code(c) {}
};
// Described in the DLL.
class RS2_EXPORT ShapeError : public std::logic_error {
public:
    using std::logic_error::logic_error;
    ~ShapeError() override;
};
class RS2_EXPORT Shape {
public:
    virtual ~Shape();
    virtual double area() const = 0;
    virtual std::string name() const;
    static int count;
};
class RS2_EXPORT Square : public Shape {
public:
    explicit Square(double s);
    double area() const override;
private:
    double side;
};
RS2_EXPORT std::unique_ptr<Shape> make_square(double s);
RS2_EXPORT double total(const std::vector<std::shared_ptr<Shape>> &v);
RS2_EXPORT const std::type_info &square_type();
RS2_EXPORT const std::type_info &inline_error_type();
RS2_EXPORT const std::type_info &hidden_type();
RS2_EXPORT void call(const std::function<void()> &f, int *destroyed);
RS2_EXPORT void fail_inline(int code);
RS2_EXPORT std::exception_ptr caught();
RS2_EXPORT void print(const std::string &s);
extern RS2_EXPORT thread_local int calls;
RS2_EXPORT int calls_here();
RS2_EXPORT int catch_from(const std::function<void()> &f);
}
#endif
