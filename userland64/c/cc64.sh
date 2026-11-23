#!/bin/sh
# Build a 64-bit rs2 program or DLL from C or C++ (docs/API64.md, "Building
# 64-bit programs in C"; the C library: docs/LIBC64.md; C++: docs/CXX64.md):
#
#   cc64.sh -o OUT.EXE [--pm] [--dll] [--static] [--rc FILE.RC] [CFLAGS...] FILE.c|.cpp|.o|.a|.DLL...
#   cc64.sh -c [--static] [CFLAGS...] FILE.c|.cpp [-o FILE.o]
#   cc64.sh --libc            (just build the C library)
#   cc64.sh --libcxx          (just build the C++ library)
#   cc64.sh --flags C|CXX     (print clang's flags: userland64/cmake)
#
# clang (any recent one: Apple's will do) compiles for x86-64 System V
# with userland64/include as the only system headers; rust-lld links with
# userland64/link64.ld as the Rust programs are, against the C library,
# built here from userland64/libc when its sources change: by default
# LIBC64.DLL (programs and DLLs import it through libc64.a, and programs
# get crt1.o, their own start-up code), with --static libc.a. Objects for
# one way must not be linked the other way (-D_RS2_LIBC_DLL decides how
# the headers declare the library's data).
# C++ sources (.cpp .cc .cxx) get userland64/cxx: LLVM's libc++, libc++abi
# and libunwind, in LIBCXX64.DLL (with --static, linked into the module);
# --cxx uses them without C++ sources on the command line (objects only).
# A 64-bit DLL on the command line is linked against: what it exports
# resolves there (the program then needs that DLL, by its file name).
# --pm marks a Presentation Manager program, --rc compiles an .RC script
# into the module's resources (os2-rc's table64).
set -e
here=$(cd "$(dirname "$0")" && pwd)
ul=$(dirname "$here")
root=$(dirname "$ul")
flags= out= pm= dll= rc= compile_only= libc_only= libcxx_only= static= cxx=
cflags= inputs=
internal=
[ "$1" = --internal-cxx ] && { internal=1; shift; }
while [ -z "$internal" ] && [ $# -gt 0 ]; do
    case "$1" in
        -o) out=$2; shift 2 ;;
        -c) compile_only=1; shift ;;
        --pm) pm=1; shift ;;
        --dll) dll=1; shift ;;
        --rc) rc=$2; shift 2 ;;
        --flags) flags=$2; shift 2 ;;
        --libc) libc_only=1; shift ;;
        --libcxx) libcxx_only=1; shift ;;
        --cxx) cxx=1; shift ;;
        --static) static=1; shift ;;
        *.cpp|*.cc|*.cxx) cxx=1; inputs="$inputs $1"; shift ;;
        *.c|*.o|*.a|*.s|*.S|*.dll|*.DLL) inputs="$inputs $1"; shift ;;
        *) cflags="$cflags $1"; shift ;;
    esac
done
usage() {
    echo "usage: cc64.sh -o OUT [--pm] [--dll] [--rc FILE.RC] [CFLAGS] FILE.c|.o|.a..." >&2
    echo "       cc64.sh -c [CFLAGS] FILE.c [-o FILE.o]" >&2
    exit 2
}
lld=${RUST_LLD:-$(ls -d "$(rustc --print sysroot)"/lib/rustlib/*/bin/rust-lld 2>/dev/null | head -1)}
ar=${LLVM_AR:-$(ls -d "$(rustc --print sysroot)"/lib/rustlib/*/bin/llvm-ar 2>/dev/null | head -1)}
nm=${LLVM_NM:-$(ls -d "$(rustc --print sysroot)"/lib/rustlib/*/bin/llvm-nm 2>/dev/null | head -1)}
# Stacks grow over a guard page, so frames of a page or more must touch
# every page (clang's stack-clash protection).
# clang, or $CLANG; C++ needs 19 or newer (libc++ 21).
clang=${CLANG:-clang}
# Not the Linux target: __linux__ would mislead portable programs.
target=x86_64-unknown-none-elf
common="--target=$target -D__RS2__ -D__OS2__ -nostdinc -fno-stack-protector -fPIC -fvisibility=hidden -fno-plt \
    -femulated-tls -Xclang -fstack-clash-protection -O2"
cc() {
    $clang $common -isystem "$ul/include" -fexceptions -fasynchronous-unwind-tables -std=gnu11 -Wall -Wno-pointer-sign "$@"
}
llvm=$ul/cxx/llvm
case "$flags" in
    C) echo "$common -isystem $ul/include -D_RS2_LIBC_DLL -fexceptions -fasynchronous-unwind-tables"; exit ;;
    CXX) echo "$common -nostdinc++ -isystem $ul/cxx/config -isystem $llvm/libcxx/include -isystem $llvm/libcxxabi/include" \
        "-isystem $ul/include -D_RS2_LIBC_DLL -fexceptions -fasynchronous-unwind-tables -fvisibility-inlines-hidden"; exit ;;
esac
cxxc() {
    $clang --driver-mode=g++ $common -nostdinc++ -isystem "$ul/cxx/config" -isystem "$llvm/libcxx/include" \
        -isystem "$llvm/libcxxabi/include" -isystem "$ul/include" \
        -fexceptions -frtti -fasynchronous-unwind-tables -fvisibility-inlines-hidden -std=gnu++20 "$@"
}
# One source, by its extension.
compile() {
    case "$1" in
        *.cpp|*.cc|*.cxx) cxxc -Wall $cflags -c "$1" -o "$2" ;;
        *) cc $cflags -c "$1" -o "$2" ;;
    esac
}

if [ -n "$internal" ]; then
    case "$1" in
        *.cpp) src=$1; obj=$2; shift 2; cxxc "$@" -c "$src" -o "$CC64_OBJ/$obj" ;;
        *) src=$1; obj=$2; shift 2; cc "$@" -c "$src" -o "$CC64_OBJ/$obj" ;;
    esac
    exit
fi

# The C library: rebuilt when a source or header is newer than it.
#   libc.a        the library, static (with crt1.o)
#   LIBC64.DLL    the library as a DLL (C:\RS2\DLL64\LIBC64.DLL)
#   libc64.a      its import library: one stub per function,
#                 "jmp *LIBC64.name@GOTPCREL(%rip)"
#   crt1.o        a program's start-up code, for use with the DLL
#   crtdll.o      a DLL's start-up code (_DLL_InitTerm: constructors)
libdir=${LIBC64_DIR:-$root/target/libc64}
lib=$libdir/libc.a
implib=$libdir/libc64.a
crt1=$libdir/crt1.o
crtdll=$libdir/crtdll.o
dllfile=$libdir/LIBC64.DLL
# Where LIBC64.DLL is linked (the loader moves it if that range is taken).
LIBC64_BASE=0x18100000
link() {
    "$lld" -flavor gnu -shared -T "$ul/link64.ld" --no-rosegment -nostdlib -z now -z norelro -Bsymbolic \
        --hash-style=sysv --gc-sections --eh-frame-hdr "$@"
}
# Imports are named MODULE.name or exported by a DLL linked against
# (check_imports FILE DLL...): anything else undefined cannot load.
check_imports() {
    f=$1
    shift
    bad=$( { for d in "$@"; do "$nm" -D --defined-only "$d" | awk '{ print "+", $NF }'; done
             "$nm" -D --undefined-only "$f" | awk '$1 == "U" { print "-", $2 }'; } |
        awk '$1 == "+" { has[$2] = 1; next } !($2 in has) && ($2 !~ /[.]/ || $2 ~ /^__emutls_/) { print "    " $2 }')
    if [ -n "$bad" ]; then
        echo "cc64.sh: $f: undefined:" >&2
        echo "$bad" >&2
        rm -f "$f"
        exit 1
    fi
}
# A DLL's name for DT_NEEDED: its file name without .DLL, in upper case.
soname() {
    basename "$1" | sed 's/\.[dD][lL][lL]$//' | tr '[:lower:]' '[:upper:]'
}
build_libc() {
    if [ -f "$lib" ] && [ -f "$implib" ] && [ -f "$dllfile" ] && [ -f "$crt1" ] && [ -f "$crtdll" ] &&
        [ -z "$(find "$ul/libc" "$ul/include" "$ul/link64.ld" "$here/cc64.sh" -newer "$lib" -type f | head -1)" ]; then
        return
    fi
    rm -rf "$libdir/obj" "$libdir/dll" "$libdir/stubs"
    mkdir -p "$libdir/obj" "$libdir/dll" "$libdir/stubs"
    for s in "$ul"/libc/*.c "$ul"/libc/*.S; do
        n=$(basename "$s" | sed 's/\.[cS]$//')
        cc -ffreestanding -fno-builtin -D_RS2_BUILDING_LIBC -Werror -c "$s" -o "$libdir/obj/$n.o"
        [ "$n" = crt1 ] && continue
        if [ "$n" = crtdll ]; then
            mv "$libdir/obj/crtdll.o" "$crtdll"
            continue
        fi
        # The DLL exports the library's functions and data (internal
        # ones are hidden in libc.h).
        cc -ffreestanding -fno-builtin -D_RS2_BUILDING_LIBC -fvisibility=default -Werror -c "$s" -o "$libdir/dll/$n.o"
    done
    cp "$libdir/obj/crt1.o" "$crt1"
    rm -f "$lib" "$implib"
    "$ar" rcs "$lib" "$libdir"/obj/*.o
    printf '.text\n.globl _start\n_start: ret\n' > "$libdir/dllstart.s"
    $clang --target=$target -c "$libdir/dllstart.s" -o "$libdir/dllstart.o"
    link -soname=LIBC64 --defsym=__image_base=$LIBC64_BASE "$libdir/dllstart.o" "$libdir"/dll/*.o -o "$dllfile"
    check_imports "$dllfile"
    # The import library: a stub for each exported function (in its own
    # member, so a program takes only what it calls). Data is imported
    # through the headers' asm labels instead.
    "$nm" -D --defined-only "$dllfile" | awk '$2 == "T" && $3 !~ /^_rs2_/ && $3 != "_start" { print $3 }' > "$libdir/exports.txt"
    # (Numbered: _exit and _Exit are one file to some file systems.)
    n=0
    while read -r f; do
        n=$((n + 1))
        printf '.section .text.%s,"ax",@progbits\n.globl %s\n.hidden %s\n.type %s,@function\n%s:\n    jmp *"LIBC64.%s"@GOTPCREL(%%rip)\n' \
            "$f" "$f" "$f" "$f" "$f" "$f" > "$libdir/stubs/$n.s"
    done < "$libdir/exports.txt"
    (cd "$libdir/stubs" && $clang --target=$target -c ./*.s)
    "$ar" rcs "$implib" "$libdir"/stubs/*.o
}
# The C++ library, for programs that use LIBC64.DLL or (static) libc.a:
# the two differ in how the C library's data is declared.
#   LIBCXX64.DLL    libc++, libc++abi and libunwind (C:\RS2\DLL64)
#   libcxx64s.a     the same, to link in
cxxlib=$libdir/libcxx64s.a
[ -n "$static" ] || cxxlib=$libdir/LIBCXX64.DLL
LIBCXX64_BASE=0x18200000
build_libcxx() {
    if [ -f "$cxxlib" ] && [ -z "$(find "$ul/cxx" "$ul/include" "$ul/link64.ld" "$here/cc64.sh" -newer "$cxxlib" -type f | head -1)" ]; then
        return
    fi
    o=$libdir/cxxobj${static:+s}
    rm -rf "$o"
    mkdir -p "$o"
    # In the DLL the annotated interface is exported.
    d="-D_LIBCXXABI_DISABLE_VISIBILITY_ANNOTATIONS -D_LIBUNWIND_DISABLE_VISIBILITY_ANNOTATIONS"
    [ -n "$static" ] || d=-D_RS2_LIBC_DLL
    cf="-ffunction-sections -fdata-sections -DNDEBUG -w $d"
    {
        for f in "$llvm"/libcxx/src/*.cpp "$llvm"/libcxx/src/ryu/*.cpp "$llvm"/libcxx/src/filesystem/*.cpp; do
            case "$f" in */libcxx/src/new.cpp) continue ;; */int128_builtins.cpp) ;; esac
            echo "$f cxx-$(basename "$f" .cpp).o $cf -std=c++23 -D_LIBCPP_BUILDING_LIBRARY -DLIBCXX_BUILDING_LIBCXXABI" \
                "-D_LIBCPP_HAS_CLOCK_GETTIME -DLIBC_NAMESPACE=__llvm_libc_common_utils -I$llvm/libcxx/src -I$llvm/libc"
        done
        for f in "$llvm"/libcxxabi/src/*.cpp; do
            case "$f" in */cxa_noexception.cpp|*/cxa_thread_atexit.cpp) continue ;; esac
            echo "$f abi-$(basename "$f" .cpp).o $cf -std=c++23 -D_LIBCXXABI_BUILDING_LIBRARY -D_LIBCPP_BUILDING_LIBRARY" \
                "-DLIBCXXABI_USE_LLVM_UNWINDER" \
                "-I$llvm/libcxxabi/src -I$llvm/libcxx/src -I$llvm/libunwind/include"
        done
        u="$cf -D_LIBUNWIND_IS_BAREMETAL -I$llvm/libunwind/include -I$llvm/libunwind/src"
        echo "$llvm/libunwind/src/libunwind.cpp unw-libunwind.o $u -std=c++17 -fno-exceptions -fno-rtti"
        for f in UnwindLevel1.c UnwindLevel1-gcc-ext.c UnwindRegistersRestore.S UnwindRegistersSave.S; do
            echo "$llvm/libunwind/src/$f unw-${f%.*}.o $u -fexceptions -fasynchronous-unwind-tables"
        done
    } > "$o/list"
    n=$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)
    # Each line of the list is one compilation.
    CC64_OBJ=$o xargs -P "$n" -L 1 "$here/cc64.sh" --internal-cxx < "$o/list"
    rm -f "$cxxlib"
    if [ -n "$static" ]; then
        "$ar" rcs "$cxxlib" "$o"/*.o
    else
        link -soname=LIBCXX64 --defsym=__image_base=$LIBCXX64_BASE "$libdir/dllstart.o" "$crtdll" "$o"/*.o "$implib" -o "$cxxlib"
        check_imports "$cxxlib"
    fi
}
build_libc
[ -n "$libc_only" ] && exit 0
if [ -n "$libcxx_only" ]; then
    build_libcxx
    exit 0
fi

[ -n "$static" ] || cflags="-D_RS2_LIBC_DLL $cflags"
if [ -n "$compile_only" ]; then
    [ -n "$inputs" ] || usage
    set -- $inputs
    [ $# -eq 1 ] || [ -z "$out" ] || usage
    for s in $inputs; do
        o=${out:-$(basename "${s%.*}").o}
        compile "$s" "$o"
    done
    exit 0
fi
[ -n "$out" ] && [ -n "$inputs" ] || usage
tmp=$(mktemp -d "${TMPDIR:-/tmp}/cc64.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
objs= dlls=
i=0
for s in $inputs; do
    case "$s" in
        *.o|*.a) objs="$objs $s" ;;
        *.dll|*.DLL) objs="$objs $s"; dlls="$dlls $s" ;;
        *)
            i=$((i + 1))
            compile "$s" "$tmp/$i.o"
            objs="$objs $tmp/$i.o"
            ;;
    esac
done
# The program type and the resources: data symbols the loader reads.
{
    if [ -n "$pm" ]; then
        printf '.section .rodata\n.globl RS2_APPTYPE\n.type RS2_APPTYPE,@object\nRS2_APPTYPE: .long 3\n'
    fi
    if [ -n "$rc" ]; then
        (cd "$root" && cargo run -q --release -p os2-rc --features host --example rc -- table64 "$(cd "$(dirname "$rc")" && pwd)/$(basename "$rc")" "$tmp/res.bin")
        printf '.section .rodata\n.globl RS2_RESOURCES\n.type RS2_RESOURCES,@object\n.p2align 2\nRS2_RESOURCES: .incbin "%s"\n' "$tmp/res.bin"
    fi
    if [ -n "$dll" ]; then
        # The ELF entry point, unused for a DLL (crtdll.o's _DLL_InitTerm
        # is its initialisation).
        printf '.text\n.globl _start\n_start: ret\n'
    fi
} > "$tmp/meta.s"
$clang --target=$target -c "$tmp/meta.s" -o "$tmp/meta.o"
if [ -n "$static" ]; then
    libs=$lib
else
    libs=$implib
    # A program's own start-up code.
    [ -n "$dll" ] || objs="$crt1 $objs"
fi
if [ -n "$cxx" ]; then
    build_libcxx
    libs="$cxxlib $libs"
    [ -n "$static" ] || dlls="$dlls $cxxlib"
fi
# What is linked in from an archive is not exported.
opts="--exclude-libs=ALL --as-needed"
[ -n "$dll" ] && opts="$opts -soname=$(soname "$out")"
# A DLL's: _DLL_InitTerm runs its constructors.
[ -n "$dll" ] && objs="$crtdll $objs"
link $opts --export-dynamic-symbol=RS2_APPTYPE --export-dynamic-symbol=RS2_RESOURCES "$tmp/meta.o" $objs $libs -o "$out"
check_imports "$out" $dlls
