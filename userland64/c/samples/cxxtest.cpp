// CXXTEST.EXE: tests C++ on rs2 (docs/CXX64.md): the language runtime
// (constructors, exceptions, RTTI, thread_local) and libc++. Prints one
// line per check and "cxxtest: N checks, M failures"; exits with M.
#include <algorithm>
#include <any>
#include <array>
#include <atomic>
#include <barrier>
#include <charconv>
#include <chrono>
#include <complex>
#include <condition_variable>
#include <coroutine>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cwchar>
#include <deque>
#include <filesystem>
#include <format>
#include <fstream>
#include <functional>
#include <future>
#include <iomanip>
#include <iostream>
#include <latch>
#include <list>
#include <map>
#include <memory>
#include <memory_resource>
#include <mutex>
#include <numeric>
#include <optional>
#include <queue>
#include <random>
#include <ranges>
#include <regex>
#include <semaphore>
#include <set>
#include <shared_mutex>
#include <sstream>
#include <string>
#include <string_view>
#include <thread>
#include <tuple>
#include <typeindex>
#include <unordered_map>
#include <variant>
#include <vector>
#include <cfenv>
#include <fcntl.h>
#include <process.h>
#include <setjmp.h>
#include <signal.h>
#include <unistd.h>
#include <pthread.h>
#include <os2.h>
#ifdef _RS2_LIBC_DLL
#include "cxxtdl2.h"
#endif

namespace fs = std::filesystem;
using namespace std::chrono_literals;
using namespace std::string_literals;

static int failures, checks;
static void check(const char *what, bool ok) {
    checks++;
    std::cout << "  " << what << ": " << (ok ? "ok" : "FAILED") << "\n";
    if (!ok)
        failures++;
}

// ---- static objects ----

static std::string order;
struct Early { Early() { early = 7; } static int early; };
int Early::early = 1;
static Early early_object;
static std::vector<int> global_vector{1, 2, 3};
struct AtExit {
    ~AtExit() {
        if (std::getenv("CXXTEST_EXIT"))
            std::cout << "static destructor" << std::endl;
    }
} at_exit;
__attribute__((init_priority(200))) static std::string prio_a = "a";
static int guarded_count;
struct Guarded { Guarded() { std::this_thread::sleep_for(20ms); guarded_count++; } };
static Guarded &guarded() { static Guarded g; return g; }

// ---- exceptions ----

struct Track {
    static int live;
    Track() { live++; }
    Track(const Track &) { live++; }
    ~Track() { live--; }
};
int Track::live;
struct Base { virtual ~Base() = default; virtual int id() const { return 1; } };
struct Left : virtual Base { int l = 2; int id() const override { return 2; } };
struct Right : virtual Base { int r = 3; };
struct Both : Left, Right { int id() const override { return 4; } };
struct MyError : std::runtime_error {
    int code;
    MyError(int c) : std::runtime_error("my error " + std::to_string(c)), code(c) {}
};
__attribute__((noinline)) static int deep(int n) {
    Track t;
    if (n == 0)
        throw MyError(42);
    return deep(n - 1) + 1;
}
static std::string nested_text(const std::exception &e) {
    std::string s = e.what();
    try {
        std::rethrow_if_nested(e);
    } catch (const std::exception &i) {
        s += "<" + nested_text(i);
    }
    return s;
}
struct InitFails {
    Track t;
    std::string s;
    InitFails() try : s((throw 5, "x")) {
    } catch (int &) {
        throw;
    }
};

static void test_exceptions() {
    try {
        deep(50);
        check("throw", false);
    } catch (const std::exception &e) {
        auto *m = dynamic_cast<const MyError *>(&e);
        check("throw through 50 frames", m && m->code == 42 && "my error 42"s == e.what());
    }
    check("destructors while unwinding", Track::live == 0);
    int got = 0;
    try { throw 7; } catch (const char *) { got = 1; } catch (int i) { got = i; }
    check("catch by type", got == 7);
    try { throw Both(); } catch (Right &r) { got = r.id() * 10 + r.r; }
    check("catch a virtual base", got == 43);
    try { try { throw MyError(1); } catch (...) { throw; } } catch (MyError &e) { got = e.code; }
    check("rethrow", got == 1);
    try {
        try { throw std::logic_error("inner"); } catch (...) { std::throw_with_nested(MyError(2)); }
    } catch (const std::exception &e) {
        check("nested exceptions", nested_text(e) == "my error 2<inner");
    }
    try { InitFails f; got = 0; } catch (int i) { got = i; }
    check("function try block", got == 5 && Track::live == 0);
    std::exception_ptr p;
    try { throw MyError(3); } catch (...) { p = std::current_exception(); }
    try { std::rethrow_exception(p); } catch (MyError &e) { got = e.code; }
    p = nullptr;
    check("exception_ptr", got == 3);
    struct Unc { int *n; ~Unc() { *n = std::uncaught_exceptions(); } };
    try { Unc u{&got}; throw 1; } catch (int) {}
    check("uncaught_exceptions", got == 1 && std::uncaught_exceptions() == 0);
    bool bad = false;
    try { std::vector<char> v; v.reserve(size_t(1) << 45); } catch (const std::bad_alloc &) { bad = true; }
    check("bad_alloc", bad);
    char *volatile huge = new (std::nothrow) char[size_t(1) << 45];
    check("nothrow new", huge == nullptr);
    Base b;
    bad = false;
    try { (void)dynamic_cast<Both &>(b); } catch (const std::bad_cast &) { bad = true; }
    check("bad_cast", bad);
    try { std::vector<int>(3).at(9); bad = false; } catch (const std::out_of_range &) { bad = true; }
    check("out_of_range", bad);
    try { (void)std::stoi("x"); bad = false; } catch (const std::invalid_argument &) { bad = true; }
    check("invalid_argument", bad);
    try { throw std::system_error(ENOENT, std::generic_category()); } catch (const std::system_error &e) {
        check("system_error", e.code() == std::errc::no_such_file_or_directory && std::strstr(e.what(), "No such file"));
    }
    int n = 0;
    for (int i = 0; i < 20000; i++)
        try { throw i; } catch (int k) { n += k == i; }
    check("20000 throws", n == 20000);
}

static jmp_buf jb;
static void on_segv(int) { longjmp(jb, 1); }
static void test_rtti() {
    Both b;
    Base *p = &b;
    check("dynamic_cast across", dynamic_cast<Right *>(p) && dynamic_cast<Right *>(p)->r == 3);
    check("typeid", typeid(*p) == typeid(Both) && std::type_index(typeid(int)) != typeid(long));
    check("type name", "4Both"s == typeid(*p).name());
    int (Base::*mf)() const = &Base::id;
    check("pointer to virtual member", (p->*mf)() == 4);
    // A signal handler's longjmp, then exceptions still work.
    signal(SIGSEGV, on_segv);
    bool hit = setjmp(jb) != 0;
    if (!hit)
        *(volatile int *)16 = 1;
    signal(SIGSEGV, SIG_DFL);
    int got = 0;
    try { throw 9; } catch (int i) { got = i; }
    check("throw after a caught SIGSEGV", hit && got == 9);
}

static void test_static() {
    check("constructors before main", Early::early == 7 && global_vector.size() == 3 && prio_a == "a");
    std::vector<std::thread> ts;
    for (int i = 0; i < 4; i++)
        ts.emplace_back([] { guarded(); });
    for (auto &t : ts)
        t.join();
    check("function static: once among threads", guarded_count == 1);
    struct alignas(256) Wide { char c[256]; };
    auto w = std::make_unique<Wide>();
    check("aligned new", (reinterpret_cast<uintptr_t>(w.get()) & 255) == 0);
}

// ---- the library ----

struct Gen {
    struct promise_type {
        int v;
        Gen get_return_object() { return {std::coroutine_handle<promise_type>::from_promise(*this)}; }
        std::suspend_always initial_suspend() { return {}; }
        std::suspend_always final_suspend() noexcept { return {}; }
        std::suspend_always yield_value(int x) { v = x; return {}; }
        void return_void() {}
        void unhandled_exception() { throw; }
    };
    std::coroutine_handle<promise_type> h;
    ~Gen() { h.destroy(); }
};
static Gen squares(int n) {
    for (int i = 1; i <= n; i++)
        co_yield i * i;
}

static void test_containers() {
    std::map<std::string, int> m;
    for (auto w : {"b", "a", "c", "a"})
        m[w]++;
    check("map", m.size() == 3 && m["a"] == 2 && m.begin()->first == "a");
    std::unordered_map<std::string, int> u;
    for (int i = 0; i < 10000; i++)
        u["k" + std::to_string(i)] = i;
    check("unordered_map", u.size() == 10000 && u["k7777"] == 7777);
    std::vector<int> v(10000);
    std::iota(v.begin(), v.end(), 0);
    std::shuffle(v.begin(), v.end(), std::mt19937(5));
    std::sort(v.begin(), v.end());
    check("shuffle and sort", std::is_sorted(v.begin(), v.end()) && v[5000] == 5000);
    std::list<int> l{3, 1, 2};
    l.sort();
    std::deque<int> d(l.begin(), l.end());
    std::priority_queue<int> q(d.begin(), d.end());
    check("list, deque, priority_queue", d[0] == 1 && q.top() == 3);
    auto r = std::views::iota(1, 20) | std::views::filter([](int i) { return i % 3 == 0; }) | std::views::transform([](int i) { return i * i; });
    check("ranges", std::ranges::distance(r) == 6 && *r.begin() == 9);
    std::variant<int, std::string> var = "text"s;
    check("variant, optional, any", std::get<std::string>(var) == "text" && std::optional<int>(4).value() == 4 && std::any_cast<double>(std::any(2.5)) == 2.5);
    std::function<int(int)> f = [k = 3](int x) { return x * k; };
    check("function", f(5) == 15);
    auto sp = std::make_shared<Both>();
    std::weak_ptr<Both> wp = sp;
    bool alive = !wp.expired();
    sp.reset();
    check("shared_ptr", alive && wp.expired());
    std::pmr::monotonic_buffer_resource res;
    std::pmr::vector<int> pv({1, 2, 3}, &res);
    check("pmr", pv.size() == 3);
    int sum = 0;
    for (Gen g = squares(4); g.h.resume(), !g.h.done();)
        sum += g.h.promise().v;
    check("coroutine", sum == 30);
}

static void test_text() {
    std::ostringstream o;
    o << std::hex << 255 << ' ' << std::setprecision(3) << 3.14159 << ' ' << std::setw(6) << std::setfill('*') << std::dec << 42 << ' ' << std::boolalpha << true;
    check("ostringstream", o.str() == "ff 3.14 ****42 true");
    std::istringstream i("12 0.5 word");
    int a;
    double d;
    std::string w;
    i >> a >> d >> w;
    check("istringstream", a == 12 && d == 0.5 && w == "word" && i.eof());
    check("format", std::format("{:>6}|{:<4}|{:#x}|{:.2f}|{}", 42, "ab", 255, 2.5, true) == "    42|ab  |0xff|2.50|true");
    check("to_string", std::to_string(-7) == "-7" && std::to_string(1.5) == "1.500000" && std::to_wstring(12) == L"12");
    check("stod, stoll", std::stod("2.5e3") == 2500 && std::stoll("-9000000000") == -9000000000LL && std::stod(L"1.25") == 1.25);
    char buf[64];
    auto tc = std::to_chars(buf, buf + 64, 0.1);
    check("to_chars", std::string(buf, tc.ptr) == "0.1");
    double back;
    auto fc = std::from_chars("1.7976931348623157e308", &"1.7976931348623157e308"[22], back);
    check("from_chars", fc.ec == std::errc() && back == 1.7976931348623157e308);
    std::smatch sm;
    std::string s = "file-123.txt";
    check("regex", std::regex_match(s, sm, std::regex(R"((\w+)-(\d+)\.txt)")) && sm[2] == "123");
    check("regex_replace", std::regex_replace("a1b22"s, std::regex("[0-9]+"), "#") == "a#b#");
    std::wstring ws = L"wide";
    std::wostringstream wo;
    wo << ws << 5;
    check("wide streams", wo.str() == L"wide5" && std::wcslen(L"abc") == 3);
    check("locale C", std::locale("").name() == "C" || std::locale().name() == "C");
    bool threw = false;
    try { std::locale l("xx_YY"); } catch (const std::runtime_error &) { threw = true; }
    check("unknown locale throws", threw);
    std::ostringstream t;
    std::tm tm{};
    tm.tm_year = 100; tm.tm_mon = 1; tm.tm_mday = 29;
    t << std::put_time(&tm, "%Y-%m-%d");
    check("put_time", t.str() == "2000-02-29");
    check("complex", std::abs(std::exp(std::complex<double>(0, M_PI)) + 1.0) < 1e-15);
    check("cmath overloads", std::pow(2.0f, 3) == 8 && std::lround(2.5f) == 3 && std::nextafter(1.0f, 2.0f) > 1.0f && std::hypot(3.0, 4.0) == 5);
    std::fesetround(FE_UPWARD);
    volatile double one = 1, three = 3;
    volatile double up = one / three;
    std::fesetround(FE_TONEAREST);
    check("fenv rounding", up > one / three);
    std::random_device rd;
    check("random_device", rd() != rd() || rd() != rd());
}

static void test_files() {
    {
        std::ofstream f("CXXTEST.TMP");
        f << "first line\n" << 42 << "\nlast\n";
        check("ofstream", bool(f));
    }
    {
        std::ifstream f("CXXTEST.TMP");
        std::string line, w;
        int n = 0;
        std::getline(f, line);
        f >> n >> w;
        check("ifstream", line == "first line" && n == 42 && w == "last");
    }
    {
        std::ofstream("CXXTEST.TMP", std::ios::binary) << "0123456789";
        std::fstream f("CXXTEST.TMP", std::ios::in | std::ios::out | std::ios::binary);
        f.seekp(5);
        f << "X";
        f.seekg(0);
        std::string all((std::istreambuf_iterator<char>(f)), {});
        check("fstream seek", all == "01234X6789");
    }
    check("missing file", !std::ifstream("NOSUCH.FIL"));
    fs::path p("C:\\OS2\\DLL\\PMWIN.DLL");
    check("path parts", p.root_name() == "C:" && p.root_directory() == "\\" && p.filename() == "PMWIN.DLL" && p.stem() == "PMWIN" && p.extension() == ".DLL" && p.is_absolute());
    check("path with /", fs::path("C:/a/b").parent_path() == "C:/a" && fs::path("dir") / "f" == "dir\\f" && fs::path("C:/a").make_preferred() == "C:\\a");
    check("lexically_normal", fs::path("C:\\a\\..\\b\\.\\c").lexically_normal() == "C:\\b\\c");
    check("relative", !fs::path("a\\b").is_absolute() && !fs::path("C:a").is_absolute() && fs::path("\\a").has_root_directory());
    std::error_code ec;
    fs::remove_all("CXXDIR", ec);
    check("create_directories", fs::create_directories("CXXDIR\\sub\\deep"));
    std::ofstream("CXXDIR\\one.txt") << "1";
    std::ofstream("CXXDIR\\sub\\two.txt") << "22";
    check("file_size, exists", fs::file_size("CXXDIR\\sub\\two.txt") == 2 && !fs::exists("CXXDIR\\none"));
    int files = 0, dirs = 0;
    for (auto &e : fs::recursive_directory_iterator("CXXDIR"))
        (e.is_directory() ? dirs : files)++;
    check("recursive_directory_iterator", files == 2 && dirs == 2);
    check("copy_file", fs::copy_file("CXXDIR\\one.txt", "CXXDIR\\three.txt", ec) && fs::file_size("CXXDIR\\three.txt") == 1);
    fs::rename("CXXDIR\\three.txt", "CXXDIR\\four.txt", ec);
    check("rename", !ec && fs::exists("CXXDIR\\four.txt"));
    fs::resize_file("CXXDIR\\four.txt", 100, ec);
    check("resize_file", !ec && fs::file_size("CXXDIR\\four.txt") == 100);
    auto when = fs::file_time_type::clock::now() - 48h;
    fs::last_write_time("CXXDIR\\four.txt", when, ec);
    auto diff = fs::last_write_time("CXXDIR\\four.txt") - when;
    check("last_write_time", !ec && diff < 3s && diff > -3s);
    check("current_path, absolute", fs::current_path().is_absolute() && fs::absolute("x") == fs::current_path() / "x");
    check("canonical", fs::canonical("CXXDIR\\sub\\..\\one.txt") == fs::current_path() / "CXXDIR\\one.txt");
    check("space", fs::space("C:\\").capacity > 1000000);
    check("remove_all", fs::remove_all("CXXDIR") == 6 && !fs::exists("CXXDIR"));
    fs::remove("CXXTEST.TMP");
    bool threw = false;
    try { (void)fs::file_size("NOSUCH.FIL"); } catch (const fs::filesystem_error &e) { threw = e.code() == std::errc::no_such_file_or_directory; }
    check("filesystem_error", threw);
}

// ---- threads ----

static std::atomic<int> tl_destroyed;
struct TL { int v = 5; ~TL() { tl_destroyed++; } };
static thread_local TL tl;
static thread_local int tl_counter = 100;

static void test_threads() {
    std::vector<std::thread> ts;
    std::mutex mx;
    long total = 0;
    for (int i = 0; i < 8; i++)
        ts.emplace_back([&] { for (int k = 0; k < 2000; k++) { std::lock_guard g(mx); total++; } });
    for (auto &t : ts)
        t.join();
    ts.clear();
    check("mutex", total == 16000);
    std::atomic<int> distinct = 0;
    for (int i = 0; i < 4; i++)
        ts.emplace_back([&, i] { tl.v = i; tl_counter += i; std::this_thread::yield(); if (tl.v == i && tl_counter == 100 + i) distinct++; });
    for (auto &t : ts)
        t.join();
    ts.clear();
    check("thread_local", distinct == 4 && tl_counter == 100);
    check("thread_local destructors", tl_destroyed == 4);
    _beginthread([](void *) { tl.v = 1; }, 0, 0, 0);
    for (int i = 0; i < 100 && tl_destroyed < 5; i++)
        std::this_thread::sleep_for(10ms);
    check("thread_local in _beginthread", tl_destroyed == 5);
    std::condition_variable cv;
    std::queue<int> q;
    int sum = 0;
    std::thread consumer([&] { for (;;) { std::unique_lock l(mx); cv.wait(l, [&] { return !q.empty(); }); int v = q.front(); q.pop(); if (v < 0) return; sum += v; } });
    for (int i = 1; i <= 500; i++) {
        { std::lock_guard g(mx); q.push(i); }
        cv.notify_one();
    }
    { std::lock_guard g(mx); q.push(-1); }
    cv.notify_one();
    consumer.join();
    check("condition_variable", sum == 125250);
    {
        std::unique_lock l(mx);
        auto t0 = std::chrono::steady_clock::now();
        bool timed = cv.wait_for(l, 100ms) == std::cv_status::timeout;
        auto dt = std::chrono::steady_clock::now() - t0;
        check("wait_for timeout", timed && dt >= 60ms && dt < 2s);
    }
    auto fut = std::async(std::launch::async, [] { std::this_thread::sleep_for(5ms); return 6 * 7; });
    check("async", fut.get() == 42);
    auto thrower = std::async(std::launch::async, []() -> int { throw MyError(8); });
    int code = 0;
    try { thrower.get(); } catch (MyError &e) { code = e.code; }
    check("exception through a future", code == 8);
    std::promise<std::string> pr;
    auto pf = pr.get_future();
    std::thread([&] { pr.set_value_at_thread_exit("done"); }).detach();
    check("promise at thread exit", pf.get() == "done");
    std::once_flag once;
    std::atomic<int> ran = 0;
    for (int i = 0; i < 4; i++)
        ts.emplace_back([&] { std::call_once(once, [&] { ran++; }); });
    for (auto &t : ts)
        t.join();
    ts.clear();
    check("call_once", ran == 1);
    std::shared_mutex sm;
    { std::shared_lock a(sm), b(sm); check("shared_mutex", !sm.try_lock()); }
    std::recursive_mutex rm;
    { std::lock_guard a(rm); std::lock_guard b(rm); }
    std::timed_mutex tm;
    tm.lock();
    bool got = true;
    std::thread([&] { got = tm.try_lock_for(30ms); }).join();
    tm.unlock();
    check("timed_mutex", !got);
    std::counting_semaphore<4> sem(0);
    std::latch la(3);
    for (int i = 0; i < 3; i++)
        ts.emplace_back([&] { sem.acquire(); la.count_down(); });
    sem.release(3);
    la.wait();
    for (auto &t : ts)
        t.join();
    ts.clear();
    check("semaphore, latch", la.try_wait());
    std::barrier bar(4, [&]() noexcept { ran++; });
    for (int i = 0; i < 4; i++)
        ts.emplace_back([&] { for (int k = 0; k < 10; k++) bar.arrive_and_wait(); });
    for (auto &t : ts)
        t.join();
    ts.clear();
    check("barrier", ran == 11);
    std::atomic<int> flag = 0;
    std::thread waker([&] { std::this_thread::sleep_for(20ms); flag = 1; flag.notify_all(); });
    flag.wait(0);
    waker.join();
    check("atomic wait", flag == 1);
    // (std::jthread is experimental in LLVM 21.)
    pthread_key_t key;
    static std::atomic<int> key_dead;
    pthread_key_create(&key, [](void *) { key_dead++; });
    pthread_t pt;
    void *res = nullptr;
    auto body = [](void *k) -> void * { pthread_setspecific(*(pthread_key_t *)k, k); return (void *)0x77; };
    check("pthread_create, join", pthread_create(&pt, nullptr, body, &key) == 0 && pthread_join(pt, &res) == 0 && res == (void *)0x77);
    check("pthread key destructor", key_dead == 1);
    check("hardware_concurrency", std::thread::hardware_concurrency() >= 1);
    // 100 threads, so that stacks and events are given back.
    int ok = 0;
    for (int i = 0; i < 100; i++)
        try { std::thread([&] { ok++; }).join(); } catch (const std::system_error &) { break; }
    check("100 threads in turn", ok == 100);
}

// ---- one C++ library for the program and its DLLs ----

#ifdef _RS2_LIBC_DLL
namespace { struct Hidden {}; }
struct Circle : shapes::Shape {
    double area() const override { return 3; }
    std::string name() const override { return "circle, " + shapes::Shape::name(); }
};
static void test_modules() {
    int got = 0, destroyed = 0;
    try { shapes::fail_inline(31); } catch (const shapes::InlineError &e) { got = e.code; }
    check("DLL's exception, inline type, caught by type", got == 31);
    got = 0;
    try { shapes::make_square(-1); } catch (const shapes::ShapeError &e) { got = !std::strcmp(e.what(), "negative side"); }
    check("DLL's exception, exported type", got == 1);
    got = 0;
    try { shapes::make_square(-1); } catch (const std::exception &) { got = 1; }
    check("DLL's exception caught as std::exception", got == 1);
    got = 0;
    try { shapes::call([] { throw 77; }, &destroyed); } catch (int i) { got = i; }
    check("exception through the DLL's frames", got == 77 && destroyed == 1);
    check("DLL catches the program's exceptions", shapes::catch_from([] { throw shapes::InlineError(12); }) == 12 &&
              shapes::catch_from([] { throw std::runtime_error("x"); }) == -1 && shapes::catch_from([] { throw 'c'; }) == -2);
    got = 0;
    try { std::rethrow_exception(shapes::caught()); } catch (const std::out_of_range &) { got = 1; }
    check("exception_ptr from the DLL", got == 1);
    check("uncaught_exceptions after", std::uncaught_exceptions() == 0);
    auto sq = shapes::make_square(3);
    check("object from the DLL", sq->area() == 9 && sq->name().size() > 40 && shapes::Shape::count == 1);
    check("dynamic_cast of the DLL's object", dynamic_cast<shapes::Square *>(sq.get()) != nullptr);
    auto &sq_ref = *sq;
    check("typeid across modules", typeid(sq_ref) == shapes::square_type() && typeid(shapes::InlineError) == shapes::inline_error_type());
    check("unnamed namespaces stay apart", typeid(Hidden) != shapes::hidden_type());
    std::vector<std::shared_ptr<shapes::Shape>> v{std::make_shared<Circle>(), std::shared_ptr<shapes::Shape>(std::move(sq))};
    check("program's class, DLL's base", shapes::total(v) == 12 && v[0]->name().starts_with("circle, shape"));
    std::cout << "  shared cout: " << std::hex;
    shapes::print("ok\n");
    std::cout << std::dec;
    check("exported thread_local", shapes::calls_here() == 11 && shapes::calls == 11);
    std::thread([&] { got = shapes::calls_here(); }).join();
    check("... in another thread", got == 11 && shapes::calls == 11);
    // The C library has unwind tables too.
    int arr[4] = {4, 3, 2, 1};
    got = 0;
    try { std::qsort(arr, 4, sizeof *arr, [](const void *, const void *) -> int { throw 5; }); } catch (int i) { got = i; }
    check("exception through qsort", got == 5);
    check("std::function from the DLL's side", [&] { int n = 0; shapes::call([&] { n = 8; }, &destroyed); return n == 8; }());
}
#else
static void test_modules() {}
#endif

static void test_processes(const char *self) {
    auto run = [&](const char *what) {
        const char *argv[] = {self, what, nullptr};
        return spawnv(P_WAIT, self, (char *const *)argv);
    };
    check("uncaught exception: terminate", run("child-throw") == 3);
    check("throw in noexcept: terminate", run("child-noexcept") == 3);
    check("exit code through exit()", run("child-exit") == 9);
    setenv("CXXTEST_EXIT", "1", 1);
    FILE *p = popen((std::string(self) + " child-exit").c_str(), "r");
    char b[64] = "";
    if (p) {
        fgets(b, sizeof b, p);
        pclose(p);
    }
    unsetenv("CXXTEST_EXIT");
    check("static destructor at exit, cout flushed", "static destructor\n"s == b);
    std::remove("CXXTDLL.FIN");
    HMODULE h = 0;
    char failed[64];
    int (*lookup)(const char *) = nullptr;
    int (*calls)() = nullptr;
    if (DosLoadModule((PSZ)failed, sizeof failed, (PCSZ) "CXXTDLL", &h) == 0) {
        DosQueryProcAddr(h, 0, (PCSZ) "dll_lookup", (PPVOID)&lookup);
        DosQueryProcAddr(h, 0, (PCSZ) "dll_calls", (PPVOID)&calls);
    }
    check("C++ DLL", lookup && lookup("two") == 2);
    check("exception inside the DLL", lookup && lookup("nine") == -1);
    check("thread_local in the DLL", calls && calls() == 2);
    DosFreeModule(h);
    std::string fin;
    std::ifstream("CXXTDLL.FIN") >> fin;
    check("DLL's static destructor at DosFreeModule", fin == "destroyed");
    std::remove("CXXTDLL.FIN");
}

int main(int argc, char **argv) {
    if (argc > 1 && !std::strncmp(argv[1], "child", 5)) {
        if (!std::strcmp(argv[1], "child-throw"))
            throw MyError(99);
        if (!std::strcmp(argv[1], "child-noexcept"))
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wexceptions" // on purpose: std::terminate
            []() noexcept { throw 1; }();
#pragma clang diagnostic pop
        std::exit(9);
    }
    std::cout << "cxxtest: C++ on rs2, libc++ " << _LIBCPP_VERSION << "\n";
    try {
        test_static();
        test_exceptions();
        test_rtti();
        test_containers();
        test_text();
        test_files();
        test_threads();
        test_modules();
        test_processes(argv[0]);
    } catch (const std::exception &e) {
        std::cout << "unexpected exception: " << e.what() << "\n";
        failures++;
    }
    std::cout << "cxxtest: " << checks << " checks, " << failures << " failures" << std::endl;
    return failures;
}
