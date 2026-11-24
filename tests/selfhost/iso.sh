#!/bin/sh
# rs2's live ISO built on rs2 (docs/SELF-HOSTING.md), all of it: the boot
# loader, the kernel, the 32-bit and 64-bit programs, the C and C++
# libraries, and the image; then rs2 booted from it. Needs what run.sh does.
#
#   iso.sh --64     (only the 64-bit programs, booted with the host's rest)
#   iso.sh          (everything: many hours in an emulator)
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
cd "$root"
t=$root/work/isotest
. "$here/on-rs2.sh"
# What xtask would download.
for d in cache firmware; do
    [ ! -d "work/$d" ] || { mkdir -p "$t/rs2/work" && cp -RL "work/$d" "$t/rs2/work/"; }
done
if [ "$1" = --64 ]; then
    say 'cargo run -p xtask -- userland64'
else
    say 'cargo run -p xtask -- iso --dist --release -o E:/rs2.iso'
fi
say 'if errorlevel 1 echo build failed'
say 'dir work\userland\64'
build
cargo xtask diskget "$t/d.img" 'rs2\work\userland\64\*' -o "$t/out/userland64"
ls "$t/out/userland64"
for f in LIBC64.DLL LIBCXX64.DLL CXXTEST.EXE STDTST64.EXE STDTRUST.EXE STDTRLIB.DLL PMWIN.DLL HELLO64.EXE; do
    [ -f "$t/out/userland64/$f" ] || { echo "missing: $f"; exit 1; }
done
# rs2 with those programs.
boot() {
    cargo xtask run --prebuilt "$t/out" --headless --timeout "$2" --mem 6G --init "RS2\\$1.EXE${4:+ $4}" > "$t/$1.log" 2>&1
    grep -aE '^FAIL|passed|checks,|failure' "$t/$1.log" | tail -5
    grep -aq "$3" "$t/$1.log"
}
boot HELLO64 60 'all tests passed'
boot CLIBTEST 90 ' 0 failures'
boot CLIBTSTS 90 ' 0 failures'
boot CXXTEST 90 ' 0 failures'
boot CXXTSTS 90 ' 0 failures'
boot STDTST64 120 ' 0 failed'
boot PMDEMO64 90 'done: 0 failure' /SELFTEST
boot PMSTD64 90 'done: 0 failure' /SELFTEST
[ "$1" != --64 ] || exit 0
cargo xtask diskget "$t/d.img" 'rs2.iso' -o "$t/out"
ls -l "$t/out/rs2.iso"
