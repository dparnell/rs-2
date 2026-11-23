#!/bin/sh
# lld on rs2 (docs/SELF-HOSTING.md): links a C program and CXXTEST there
# with LLD.EXE, compares them with what the host's lld of the same version
# makes, and runs them. Needs userland64/llvm/build.sh done, and
# `ninja -C work/llvm/host lld`.
#
# Changes work/modules.txt for the run and puts it back.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
cd "$root"
cc=userland64/c/cc64.sh
llvm=${LLVM_DIR:-work/llvm}
t=work/lld64test
mkdir -p $t
$cc -c "$here/hi.c" -o $t/hi.o
$cc -c userland64/c/samples/cxxtest.cpp -o $t/cxxtest.o
$cc -o $t/CMP64.EXE "$here/cmp.c"
$cc --dll -o $t/CXXTDL2.DLL userland64/c/samples/cxxtdl2.cpp
cp target/libc64/crt1.o target/libc64/libc64.a target/libc64/LIBCXX64.DLL userland64/link64.ld target/llvm64/LLD.EXE $t/
f="-shared -T link64.ld --no-rosegment -nostdlib -z now -z norelro -Bsymbolic --hash-style=sysv --gc-sections --eh-frame-hdr --exclude-libs=ALL --as-needed"
c="crt1.o hi.o libc64.a"
x="crt1.o cxxtest.o CXXTDL2.DLL LIBCXX64.DLL libc64.a"
case "$llvm" in /*) ;; *) llvm=$root/$llvm ;; esac
(cd $t && "$llvm/host/bin/ld.lld" $f $c -o hihost.exe && "$llvm/host/bin/ld.lld" $f $x -o cxhost.exe)
cat > $t/link.cmd <<EOF
@echo off
cd \\LT
LLD -flavor gnu $f $c -o HI.EXE
CMP64 HI.EXE hihost.exe
HI.EXE a b
LLD -flavor gnu $f $x -o CX.EXE
CMP64 CX.EXE cxhost.exe
CX.EXE
EOF
cp work/modules.txt $t/modules.save
trap 'cp $t/modules.save work/modules.txt' EXIT
for n in hi.o cxxtest.o CMP64.EXE CXXTDL2.DLL crt1.o libc64.a LIBCXX64.DLL link64.ld LLD.EXE hihost.exe cxhost.exe link.cmd; do
    printf '%s -> LT\\%s\n' "$t/$n" "$n"
done >> work/modules.txt
cargo xtask run --headless --timeout 90 --mem 4G --init 'OS2\CMD.EXE /C C:\LT\LINK.CMD' > $t/log 2>&1
grep -E 'same|differ|cannot|hello|cxxtest:|LLD|FAIL' $t/log
[ "$(grep -c 'are the same' $t/log)" = 2 ] && grep -q 'linked on rs2, 2 arg' $t/log && grep -q ' 0 failures' $t/log
