#!/bin/sh
# The native 64-bit C and C++ toolchain for rs2 as a directory tree, from
# what build.sh has built (docs/SELF-HOSTING.md):
#
#   sdk.sh [DIR]              (target/sdk64 by default)
#
#   DIR/bin      CLANG.EXE, LLD.EXE, LLVM-AR.EXE, clang++.EXE (runs CLANG.EXE)
#   DIR/include  rs2's C headers and OS/2 headers; c++/: libc++'s
#   DIR/lib      start-up code, import library, LIBCXX64.DLL, link64.ld,
#                static libraries; clang/VER/include: clang's headers
#   DIR.txt      the files as a manifest for `cargo xtask disk --hpfs`
#
# CLANG.EXE finds the rest relative to itself. The C++ headers' long names
# need HPFS or JFS. Without the .EXE files (work/llvm/host's clang with
# --target=x86_64-unknown-rs2 --sysroot=DIR) it is a cross SDK.
set -e
here=$(cd "$(dirname "$0")" && pwd)
ul=$(dirname "$here")
root=$(dirname "$ul")
out=${1:-$root/target/sdk64}
llvm=${LLVM_DIR:-$root/work/llvm}
cxx=$ul/cxx/llvm
cc=$ul/c/cc64.sh
lib=$root/target/libc64
$cc --libc
$cc --libcxx
$cc --static --libcxx
rm -rf "$out"
mkdir -p "$out/bin" "$out/lib" "$out/include/c++/config" "$out/include/c++/v1" "$out/include/c++/abi"
for f in CLANG LLD LLVM-AR; do
    [ ! -f "$root/target/llvm64/$f.EXE" ] || cp "$root/target/llvm64/$f.EXE" "$out/bin/"
done
$cc -o "$out/bin/clang++.EXE" "$here/clangxx.c"
cp -R "$ul/include/." "$out/include/"
cp "$ul"/cxx/config/* "$out/include/c++/config/"
cp -R "$cxx/libcxx/include/." "$out/include/c++/v1/"
cp -R "$cxx/libcxxabi/include/." "$out/include/c++/abi/"
if [ -f "$root/target/llvm64/clang/version" ]; then
    d=$out/lib/clang/$(cat "$root/target/llvm64/clang/version")
    mkdir -p "$d"
    cp -R "$root/target/llvm64/clang/include" "$d/"
fi
res=$(ls -d "$llvm"/rs2/lib/clang/*/include "$llvm"/host/lib/clang/*/include 2> /dev/null | head -1) || true
[ -z "$res" ] || [ -d "$out/lib/clang" ] || { d=$out/lib/clang/$(basename "$(dirname "$res")"); mkdir -p "$d"; cp -RL "$res" "$d/"; }
cp "$lib/crt1.o" "$lib/crtdll.o" "$lib/dllstart.o" "$lib/libc64.a" "$lib/libc.a" "$lib/libcxx64s.a" "$lib/LIBCXX64.DLL" \
    "$ul/link64.ld" "$out/lib/"
printf '.section .rodata\n.globl RS2_APPTYPE\n.type RS2_APPTYPE,@object\nRS2_APPTYPE: .long 3\n' |
    ${CLANG:-clang} --target=x86_64-unknown-none-elf -x assembler -c - -o "$out/lib/pmapp.o"
(cd "$out" && find . -type f | sed 's|^\./||' | sort) | while read -r f; do
    printf '%s -> sdk\\%s\n' "$out/$f" "$(echo "$f" | tr / '\\')"
done > "$out.txt"
wc -l "$out.txt"
du -sh "$out"
