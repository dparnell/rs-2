#!/bin/sh
# rustc on rs2 (docs/SELF-HOSTING.md): compiles, links and runs programs,
# a proc macro, a C DLL and a large crate there, and compares objects with
# what the installed rustc makes. Needs userland64/rustc/build.sh done
# (and clang's, userland64/llvm/build.sh).
#
# The toolchain and the sources are on an HPFS disk (drive E:) made here.
# Changes work/disk.txt for the run and puts it back.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
cd "$root"
rdir=${RUSTC_DIR:-$root/work/rustc}
t=$root/work/rustc64test
rm -rf "$t"
mkdir -p "$t/src" "$t/host"
userland64/rustc/sdk.sh "$t/sdk" > /dev/null
userland64/c/cc64.sh -o "$t/CMP64.EXE" tests/lld64/cmp.c
cp "$here"/*.rs "$t/src/"
cp -R "$rdir"/rustc-1.100.0-src/vendor/regex-syntax-0.8.11/src "$t/src/resyn"
cmd=$t/src/build.cmd
printf '@echo off\r\nE:\r\ncd \\src\r\nset PATH=E:\\sdk\\bin;%%PATH%%\r\nset BEGINLIBPATH=E:\\sdk\\bin;E:\\src\r\nset TMPDIR=E:\\tmp\r\n' > "$cmd"
say() { printf '%s\r\n' "$1" >> "$cmd"; }
n=0
# obj ARGS OUTPUT: an object, compared with the host's (with the same
# sysroot: rustup's may have std's source, whose path gets into objects)
obj() {
    (cd "$t/src" && rustc --sysroot "$t/sdk" --target x86_64-unknown-linux-gnu --remap-path-prefix "$t/src=/src" $1 --emit obj -o "../host/$2")
    say "rustc --remap-path-prefix E:/src=/src $1 --emit obj -o $2"
    say "\\CMP64 $2 \\host\\$2"
    n=$((n + 1))
}
say 'echo begin'
say 'rustc -V'
say 'rustc -O .\hello.rs -o HELLOR.EXE'
say 'HELLOR a b'
say 'rustc --crate-type proc-macro pm.rs'
say 'rustc usepm.rs --extern pm=libpm.so -o USEPM.EXE'
say 'USEPM'
say 'rustc -g --crate-type cdylib cdy.rs'
say 'copy libcdy.so CDY.DLL'
say 'rustc usecdy.rs -L . -o USECDY.EXE'
say 'USECDY'
say 'rustc bad.rs'
obj "-O -C codegen-units=1 hello.rs" hello.o
obj "-C codegen-units=1 -g hello.rs" hellog.o
obj "-O -C codegen-units=1 --crate-type cdylib cdy.rs" cdy.o
obj "-O -C codegen-units=1 --cap-lints allow --edition 2021 --crate-type lib --crate-name regex_syntax resyn/lib.rs" resyn.o
say 'rustc --cap-lints allow --edition 2021 --crate-type lib --crate-name regex_syntax resyn/lib.rs'
say 'dir libregex_syntax.rlib'
say 'echo end'
[ ! -f work/disk.txt ] || cp work/disk.txt "$t/disk.save"
trap '[ ! -f $t/disk.save ] || cp $t/disk.save work/disk.txt' EXIT
{
    cat "$t/sdk.txt"
    printf '%s -> CMP64.EXE\n' "$t/CMP64.EXE"
    echo "$here/hello.rs -> tmp\\keep"
    (cd "$t" && find src host -type f) | while read -r f; do printf '%s -> %s\n' "$t/$f" "$(echo "$f" | tr / '\\')"; done
} > work/disk.txt
cargo xtask disk --hpfs --size 800 -o "$t/d.img" > /dev/null
cargo xtask run --headless --timeout "${TIMEOUT:-400}" --mem 6G --hdd "$t/d.img" --init 'OS2\CMD.EXE /C E:\src\build.cmd' > "$t/log" 2>&1
sed -n '/^begin/,$p' "$t/log"
has() { grep -aq "$1" "$t/log" || { echo "run.sh: missing: $1" >&2; exit 1; }; }
has 'hello: rust on rs2 \["a", "b"\]'
has 'macro says 42 Thing'
has 'from the DLL: 672'
has 'caught: true'
has 'error\[E0308\]'
has 'libregex_syntax.rlib'
[ "$(grep -ac 'are the same' "$t/log")" = "$n" ]
