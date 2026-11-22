#!/bin/sh
# Build a 64-bit rs2 program or DLL from C (docs/API64.md, "Building
# 64-bit programs in C"; the C library: docs/LIBC64.md):
#
#   cc64.sh -o OUT.EXE [--pm] [--dll] [--static] [--rc FILE.RC] [CFLAGS...] FILE.c|FILE.o|FILE.a...
#   cc64.sh -c [--static] [CFLAGS...] FILE.c [-o FILE.o]
#   cc64.sh --libc            (just build the C library)
#
# clang (any recent one: Apple's will do) compiles for x86-64 System V
# with userland64/include as the only system headers; rust-lld links with
# userland64/link64.ld as the Rust programs are, against the C library,
# built here from userland64/libc when its sources change: by default
# LIBC64.DLL (programs and DLLs import it through libc64.a, and programs
# get crt1.o, their own start-up code), with --static libc.a. Objects for
# one way must not be linked the other way (-D_RS2_LIBC_DLL decides how
# the headers declare the library's data).
# --pm marks a Presentation Manager program, --rc compiles an .RC script
# into the module's resources (os2-rc's table64).
set -e
here=$(cd "$(dirname "$0")" && pwd)
ul=$(dirname "$here")
root=$(dirname "$ul")
out= pm= dll= rc= compile_only= libc_only= static=
cflags= inputs=
while [ $# -gt 0 ]; do
    case "$1" in
        -o) out=$2; shift 2 ;;
        -c) compile_only=1; shift ;;
        --pm) pm=1; shift ;;
        --dll) dll=1; shift ;;
        --rc) rc=$2; shift 2 ;;
        --libc) libc_only=1; shift ;;
        --static) static=1; shift ;;
        *.c|*.o|*.a|*.s|*.S) inputs="$inputs $1"; shift ;;
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
cc() {
    clang --target=x86_64-unknown-linux-gnu -nostdinc -isystem "$ul/include" -fno-stack-protector -fPIC \
        -fvisibility=hidden -fno-asynchronous-unwind-tables -fno-plt -O2 -std=gnu11 -Wall -Wno-pointer-sign "$@"
}

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
        --hash-style=sysv --gc-sections "$@"
}
build_libc() {
    if [ -f "$lib" ] && [ -f "$implib" ] && [ -f "$dllfile" ] && [ -f "$crt1" ] && [ -f "$crtdll" ] &&
        [ -z "$(find "$ul/libc" "$ul/include" "$here/cc64.sh" -newer "$lib" -type f | head -1)" ]; then
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
    clang --target=x86_64-unknown-linux-gnu -c "$libdir/dllstart.s" -o "$libdir/dllstart.o"
    link --defsym=__image_base=$LIBC64_BASE "$libdir/dllstart.o" "$libdir"/dll/*.o -o "$dllfile"
    # The import library: a stub for each exported function (in its own
    # member, so a program takes only what it calls). Data is imported
    # through the headers' asm labels instead.
    "$nm" -D --defined-only "$dllfile" | awk '$2 == "T" && $3 !~ /^_rs2_/ && $3 != "_start" { print $3 }' > "$libdir/exports.txt"
    while read -r f; do
        printf '.section .text.%s,"ax",@progbits\n.globl %s\n.hidden %s\n.type %s,@function\n%s:\n    jmp *"LIBC64.%s"@GOTPCREL(%%rip)\n' \
            "$f" "$f" "$f" "$f" "$f" "$f" > "$libdir/stubs/$f.s"
    done < "$libdir/exports.txt"
    (cd "$libdir/stubs" && clang --target=x86_64-unknown-linux-gnu -c ./*.s)
    "$ar" rcs "$implib" "$libdir"/stubs/*.o
}
build_libc
[ -n "$libc_only" ] && exit 0

[ -n "$static" ] || cflags="-D_RS2_LIBC_DLL $cflags"
if [ -n "$compile_only" ]; then
    [ -n "$inputs" ] || usage
    set -- $inputs
    [ $# -eq 1 ] || [ -z "$out" ] || usage
    for s in $inputs; do
        o=${out:-$(basename "$s" | sed 's/\.[cSs]$/.o/')}
        cc $cflags -c "$s" -o "$o"
    done
    exit 0
fi
[ -n "$out" ] && [ -n "$inputs" ] || usage
tmp=$(mktemp -d "${TMPDIR:-/tmp}/cc64.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
objs=
i=0
for s in $inputs; do
    case "$s" in
        *.o|*.a) objs="$objs $s" ;;
        *)
            i=$((i + 1))
            cc $cflags -c "$s" -o "$tmp/$i.o"
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
clang --target=x86_64-unknown-linux-gnu -c "$tmp/meta.s" -o "$tmp/meta.o"
if [ -n "$static" ]; then
    libs=$lib
else
    libs=$implib
    # A program's own start-up code.
    [ -n "$dll" ] || objs="$crt1 $objs"
fi
# A DLL's: _DLL_InitTerm runs its constructors.
[ -n "$dll" ] && objs="$crtdll $objs"
link --export-dynamic-symbol=RS2_APPTYPE --export-dynamic-symbol=RS2_RESOURCES "$tmp/meta.o" $objs $libs -o "$out"
