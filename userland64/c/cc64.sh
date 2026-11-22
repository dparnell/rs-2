#!/bin/sh
# Build a 64-bit rs2 program or DLL from C (docs/API64.md, "Building
# 64-bit programs in C"):
#
#   cc64.sh -o OUT.EXE [--pm] [--dll] [--rc FILE.RC] [CFLAGS...] FILE.c...
#
# clang (any recent one: Apple's will do) compiles for x86-64 System V,
# freestanding, with userland64/include on the include path; rust-lld
# links with userland64/link64.ld as the Rust programs are, crt0.c first
# (programs only). --pm marks a Presentation Manager program, --rc compiles
# an .RC script into the module's resources (os2-rc's table64).
set -e
here=$(cd "$(dirname "$0")" && pwd)
ul=$(dirname "$here")
root=$(dirname "$ul")
out= pm= dll= rc=
cflags= srcs=
while [ $# -gt 0 ]; do
    case "$1" in
        -o) out=$2; shift 2 ;;
        --pm) pm=1; shift ;;
        --dll) dll=1; shift ;;
        --rc) rc=$2; shift 2 ;;
        *.c) srcs="$srcs $1"; shift ;;
        *) cflags="$cflags $1"; shift ;;
    esac
done
[ -n "$out" ] && [ -n "$srcs" ] || { echo "usage: cc64.sh -o OUT [--pm] [--dll] [--rc FILE.RC] FILE.c..." >&2; exit 2; }
lld=${RUST_LLD:-$(ls -d "$(rustc --print sysroot)"/lib/rustlib/*/bin/rust-lld 2>/dev/null | head -1)}
tmp=$(mktemp -d "${TMPDIR:-/tmp}/cc64.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
cc() {
    clang --target=x86_64-unknown-linux-gnu -ffreestanding -fno-stack-protector -fPIC \
        -fvisibility=hidden -fno-asynchronous-unwind-tables -O2 -std=c11 -Wall -Wno-pointer-sign \
        -I "$ul/include" "$@"
}
objs=
if [ -z "$dll" ]; then
    cc -fno-builtin -c "$here/crt0.c" -o "$tmp/crt0.o"
    objs="$tmp/crt0.o"
fi
i=0
for s in $srcs; do
    i=$((i + 1))
    cc $cflags -c "$s" -o "$tmp/$i.o"
    objs="$objs $tmp/$i.o"
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
        printf '.text\n.globl _start\n_start: ret\n'
    fi
} > "$tmp/meta.s"
clang --target=x86_64-unknown-linux-gnu -c "$tmp/meta.s" -o "$tmp/meta.o"
"$lld" -flavor gnu -shared -T "$ul/link64.ld" --no-rosegment -nostdlib -z now -z norelro -Bsymbolic \
    --hash-style=sysv --gc-sections --export-dynamic-symbol=RS2_APPTYPE --export-dynamic-symbol=RS2_RESOURCES \
    $objs "$tmp/meta.o" -o "$out"
