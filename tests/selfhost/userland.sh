#!/bin/sh
# rs2's 32-bit userland built on rs2 (docs/SELF-HOSTING.md): compiled,
# linked and converted to LX there, then rs2 booted with it. Needs what
# run.sh does.
#
#   userland.sh --std             (userland-std's programs only, not converted)
#   userland.sh [PACKAGE...]      (userland/'s packages; all of them and
#                                 userland-std's programs by default: hours
#                                 in an emulator)
#
# The toolchain and the committed sources are on an HPFS disk (drive E:)
# made here. Changes work/disk.txt for the run and puts it back.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
cd "$root"
t=$root/work/userlandtest
. "$here/on-rs2.sh"
if [ "$1" = --std ]; then
    say 'cd userland-std'
    say 'cargo build --release'
else
    say "cargo run -p xtask -- userland $*"
fi
say 'if errorlevel 1 echo build failed'
say 'dir work\userland'
build
[ "$1" != --std ] || exit 0
cargo xtask diskget "$t/d.img" 'rs2\work\userland\*' -o "$t/out/userland"
ls "$t/out/userland" | wc -l
[ $# = 0 ] || exit 0
# Everything, userland-std's programs too: STDTEST must be this build's.
for f in STDTEST.EXE APITEST.EXE WIN16.DLL WEB.EXE WEBTEXT.EXE; do [ -f "$t/out/userland/$f" ] || { echo "missing: $f"; exit 1; }; done
# rs2 with what it built.
boot() {
    cargo xtask run --prebuilt "$t/out" --headless --timeout "$2" --mem 6G --init "RS2\\$1.EXE" > "$t/$1.log" 2>&1
    grep -aE '^FAIL|passed|checks,|w16test done|failure' "$t/$1.log" | tail -5
    grep -aq "$3" "$t/$1.log"
}
boot STDTEST 120 ' 0 failed'
boot W16TEST 200 'done: 0 failure'
boot APITEST 120 'done: 0 failure'
