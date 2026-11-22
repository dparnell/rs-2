#!/bin/sh
# Build a 64-bit rs2 program or DLL from C (docs/API64.md, "Building
# 64-bit programs in C"; the C library: docs/LIBC64.md):
#
#   cc64.sh -o OUT.EXE [--pm] [--dll] [--rc FILE.RC] [CFLAGS...] FILE.c|FILE.o|FILE.a...
#   cc64.sh -c [CFLAGS...] FILE.c [-o FILE.o]
#   cc64.sh --libc            (just build the C library)
#
# clang (any recent one: Apple's will do) compiles for x86-64 System V
# with userland64/include as the only system headers; rust-lld links with
# userland64/link64.ld as the Rust programs are, against the C library
# (libc.a, built here from userland64/libc when its sources change).
# --pm marks a Presentation Manager program, --rc compiles an .RC script
# into the module's resources (os2-rc's table64).
set -e
here=$(cd "$(dirname "$0")" && pwd)
ul=$(dirname "$here")
root=$(dirname "$ul")
out= pm= dll= rc= compile_only= libc_only=
cflags= inputs=
while [ $# -gt 0 ]; do
    case "$1" in
        -o) out=$2; shift 2 ;;
        -c) compile_only=1; shift ;;
        --pm) pm=1; shift ;;
        --dll) dll=1; shift ;;
        --rc) rc=$2; shift 2 ;;
        --libc) libc_only=1; shift ;;
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
cc() {
    clang --target=x86_64-unknown-linux-gnu -nostdinc -isystem "$ul/include" -fno-stack-protector -fPIC \
        -fvisibility=hidden -fno-asynchronous-unwind-tables -fno-plt -O2 -std=gnu11 -Wall -Wno-pointer-sign "$@"
}

# The C library: rebuilt when a source or header is newer than it.
libdir=${LIBC64_DIR:-$root/target/libc64}
lib=$libdir/libc.a
build_libc() {
    if [ -f "$lib" ] && [ -z "$(find "$ul/libc" "$ul/include" -newer "$lib" -type f | head -1)" ]; then
        return
    fi
    mkdir -p "$libdir/obj"
    rm -f "$libdir"/obj/*.o
    for s in "$ul"/libc/*.c "$ul"/libc/*.S; do
        o="$libdir/obj/$(basename "$s" | sed 's/\.[cS]$/.o/')"
        cc -ffreestanding -fno-builtin -Werror -c "$s" -o "$o"
    done
    rm -f "$lib"
    "$ar" rcs "$lib" "$libdir"/obj/*.o
}
build_libc
[ -n "$libc_only" ] && exit 0

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
        # A DLL has no C start-up: the library's state starts as it is
        # initialised in its data.
        printf '.text\n.globl _start\n_start: ret\n'
    fi
} > "$tmp/meta.s"
clang --target=x86_64-unknown-linux-gnu -c "$tmp/meta.s" -o "$tmp/meta.o"
"$lld" -flavor gnu -shared -T "$ul/link64.ld" --no-rosegment -nostdlib -z now -z norelro -Bsymbolic \
    --hash-style=sysv --gc-sections --export-dynamic-symbol=RS2_APPTYPE --export-dynamic-symbol=RS2_RESOURCES \
    "$tmp/meta.o" $objs "$lib" -o "$out"
