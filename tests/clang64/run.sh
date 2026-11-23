#!/bin/sh
# clang on rs2 (docs/SELF-HOSTING.md): compiles and links C and C++ there,
# compares the results with what the host's clang of the same build makes,
# and runs them. Needs userland64/llvm/build.sh done, and
# `ninja -C work/llvm/host clang lld`.
#
# The toolchain and the sources are on an HPFS disk (drive E:) made here.
# Changes work/disk.txt for the run and puts it back.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
cd "$root"
llvm=${LLVM_DIR:-work/llvm}
case "$llvm" in /*) ;; *) llvm=$root/$llvm ;; esac
t=$root/work/clang64test
rm -rf "$t"
mkdir -p "$t/src/libc" "$t/host/libc"
userland64/llvm/sdk.sh "$t/sdk" > /dev/null
userland64/c/cc64.sh -o "$t/CMP64.EXE" tests/lld64/cmp.c
cp "$here"/*.c "$here"/*.cpp userland64/c/samples/cxxtest.cpp userland64/c/samples/cxxtdl2.cpp userland64/c/samples/pmhello.c userland64/c/samples/cxxtdl2.h "$t/src/"
cp userland64/libc/*.c userland64/libc/*.S userland64/libc/*.h "$t/src/libc/"
clang="$llvm/host/bin/clang --target=x86_64-unknown-rs2 --sysroot=$t/sdk -resource-dir $t/sdk/lib/clang/21"
cmd=$t/src/build.cmd
printf '@echo off\r\nE:\r\ncd \\src\r\nset PATH=E:\\sdk\\bin;%%PATH%%\r\nset TMP=E:\\tmp\r\nset TMPDIR=E:\\tmp\r\n' > "$cmd"
say() { printf '%s\r\n' "$1" >> "$cmd"; }
# step ARGS OUTPUT: the same on both, then compare
step() {
    (cd "$t/src" && $clang $1 -o "../host/$2")
    say "clang $1 -o $2"
    say "\\CMP64 $2 \\host\\$2"
}
cxx="--driver-mode=g++ -std=gnu++20"
say 'echo begin'
step "-O2 hi.c" HI.EXE
say 'HI.EXE a b'
step "-O2 -Wno-pointer-sign -mwindows pmhello.c" PMHELLO.EXE
step "$cxx -O2 -shared cxxtdl2.cpp" CXXTDL2.DLL
step "$cxx -O2 -c cxxtest.cpp" cxxtest.o
say "clang $cxx cxxtest.o CXXTDL2.DLL -o CX.EXE"
(cd "$t/host" && $clang $cxx cxxtest.o CXXTDL2.DLL -o CX.EXE)
say "\\CMP64 CX.EXE \\host\\CX.EXE"
say 'CX.EXE'

n=5
for f in "$t"/src/libc/*.c; do
    b=$(basename "$f" .c)
    step "-O2 -std=gnu11 -ffreestanding -fno-builtin -D_RS2_BUILDING_LIBC -fvisibility=default -c libc/$b.c" "libc/$b.o"
    n=$((n + 1))
done
say 'set TMP='
say 'set TMPDIR='
say 'clang++ -fno-exceptions "-DWHAT=clang++ ok" plus.cpp -o PLUS.EXE'
say 'PLUS.EXE'
say 'echo int foo(); int main(){return foo();} > u.c'
say 'clang u.c -o U.EXE'
say 'echo end'
echo "$n" > "$t/count"
[ ! -f work/disk.txt ] || cp work/disk.txt "$t/disk.save"
trap '[ ! -f $t/disk.save ] || cp $t/disk.save work/disk.txt' EXIT
{
    cat "$t/sdk.txt"
    printf '%s -> CMP64.EXE\n' "$t/CMP64.EXE"
    echo "$here/hi.c -> tmp\\keep"
    (cd "$t" && find src host -type f) | while read -r f; do printf '%s -> %s\n' "$t/$f" "$(echo "$f" | tr / '\\')"; done
} > work/disk.txt
cargo xtask disk --hpfs --size 400 -o "$t/d.img" > /dev/null
cargo xtask run --headless --timeout "${TIMEOUT:-360}" --mem 4G --hdd "$t/d.img" --init 'OS2\CMD.EXE /C E:\src\build.cmd' > "$t/log" 2>&1
grep -aE 'same|differ|cannot|hello|cxxtest:|clang\+\+ ok|error|LLD|FAIL|begin|^end' "$t/log"
[ "$(grep -ac 'are the same' "$t/log")" = "$n" ] && grep -aq 'linked on rs2, 2 arg' "$t/log" && grep -aq ' 0 failures' "$t/log" &&
    grep -aq 'undefined symbol: foo' "$t/log" && grep -aq '^clang++ ok 2' "$t/log"
