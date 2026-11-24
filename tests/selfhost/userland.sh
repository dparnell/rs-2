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
rm -rf "$t"
mkdir -p "$t/rs2"
userland64/rustc/sdk.sh "$t/sdk" > /dev/null
git archive HEAD Cargo.toml Cargo.lock .cargo boot kernel crates tools xtask userland userland-std userland64/os2 userland64/Cargo.toml \
    userland64/Cargo.lock userland64/hello64 userland64/pmwin userland64/pmdemo64 | tar xf - -C "$t/rs2"
(cd "$t/rs2" && cargo vendor --locked -s userland/Cargo.toml -s userland-std/Cargo.toml vendor > /dev/null 2>&1)
printf '\n[source.crates-io]\nreplace-with = "vendored"\n[source.vendored]\ndirectory = "vendor"\n' >> "$t/rs2/.cargo/config.toml"
sed -e 's|^CC_i686.*|CC_i686_unknown_linux_gnu = "E:/sdk/bin/CLANG.EXE --target=i686-unknown-linux-gnu -ffreestanding -fno-stack-protector -isystem E:/sdk/lib/rs2-32/include"|' \
    -e 's|^AR_i686.*|AR_i686_unknown_linux_gnu = "E:/sdk/bin/LLVM-AR.EXE"|' userland-std/.cargo/config.toml > "$t/rs2/userland-std/.cargo/config.toml"
cmd=$t/build.cmd
printf '@echo off\r\nE:\r\ncd \\rs2\r\nset PATH=E:\\sdk\\bin;%%PATH%%\r\nset BEGINLIBPATH=E:\\sdk\\bin\r\nset TMPDIR=E:\\tmp\r\nset CARGO_HOME=E:\\cargo\r\nset CARGO_INCREMENTAL=0\r\nset CARGO_NET_OFFLINE=true\r\n' > "$cmd"
say() { printf '%s\r\n' "$1" >> "$cmd"; }
# userland-std's config names shell scripts for these (and for the C compiler
# above, as CMD makes variables' names upper case).
say 'set CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=rs2-ld.exe'
say 'set CARGO_TARGET_I686_UNKNOWN_LINUX_GNU_LINKER=rs2-ld32.exe'
say 'echo begin'
if [ "$1" = --std ]; then
    say 'cd userland-std'
    say 'cargo build --release'
else
    say "cargo run -p xtask -- userland $*"
fi
say 'if errorlevel 1 echo build failed'
say 'dir work\userland'
say 'echo end'
[ ! -f work/disk.txt ] || cp work/disk.txt "$t/disk.save"
trap '[ ! -f $t/disk.save ] || cp $t/disk.save work/disk.txt' EXIT
{
    cat "$t/sdk.txt"
    echo "$here/userland.sh -> tmp\\keep"
    echo "$cmd -> build.cmd"
    (cd "$t" && find rs2 -type f) | while read -r f; do printf '%s -> %s\n' "$t/$f" "$(echo "$f" | tr / '\\')"; done
} > work/disk.txt
cargo xtask disk --hpfs --size 3000 -o "$t/d.img" > /dev/null
cargo xtask run --headless --timeout "${TIMEOUT:-9000}" --mem 8G --smp "${SMP:-8}" --hdd "$t/d.img" --init 'OS2\CMD.EXE /C E:\build.cmd' > "$t/log" 2>&1
sed -n '/^begin/,$p' "$t/log" | tr '\r' '\n' | grep -av Building
grep -aq '^end' "$t/log"
if grep -aqE '^error|^build failed' "$t/log"; then exit 1; fi
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
