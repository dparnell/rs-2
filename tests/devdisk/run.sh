#!/bin/sh
# The developer disk (docs/DEVDISK.md): installed on rs2 from its CD image
# onto an empty HPFS disk (D: beside a
# virtio CD), twice; the samples and a new cargo project built
# and run there with nothing but DEVENV.CMD's settings.
#
#   run.sh [ISO]         (work/rs2-dev.iso; userland64/devdisk/build.sh)
#   run.sh --rs2 [ISO]   also builds rs2's 64-bit programs from the disk's sources
#   run.sh --iso [ISO]   the whole live ISO instead (over an hour)
#
# Changes work/disk.txt for the run and puts it back.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
cd "$root"
rs2=
case "$1" in --rs2 | --iso) rs2=$1; shift ;; esac
iso=${1:-$root/work/rs2-dev.iso}
t=$root/work/devdisktest
rm -rf "$t"
mkdir -p "$t"
cmd=$t/t.cmd
: > "$cmd"
say() { printf '%s\r\n' "$1" >> "$cmd"; }
say '@echo off'
say 'echo begin'
# The CD's letter: the first with the disk's archive.
for d in E F G H; do say "if exist $d:\\SDK.ZIP set CD=$d:"; done
say '%CD%'
say '%CD%\INSTALL /LIST'
say '%CD%\INSTALL /YES /DIR:C:\X'
say '%CD%\INSTALL /YES /DRIVE:D /SOURCE'
say 'echo second'
say '%CD%\INSTALL /YES /DIR:D:\RS2DEV'
say 'type C:\CONFIG.SYS'
say 'call D:\RS2DEV\DEVENV.CMD'
say 'type D:\RS2DEV\src\userland-std\.cargo\config.toml'
say 'call D:\RS2DEV\samples\BUILD.CMD'
say 'c\HELLO a b'
say 'cd c'
say 'USEMYLIB'
say 'PMHELLO /SELFTEST'
say 'cd ..'
say 'cpp\HELLO'
say 'rust\hello\target\release\hello'
say 'dir rust\pmhello\target\release\pmhello.exe'
say 'cd D:\RS2DEV'
say 'cargo new fresh'
say 'cd fresh'
say 'cargo run'
say 'echo samples done'
if [ "$rs2" = --iso ]; then
    say 'call D:\RS2DEV\src\BUILD.CMD'
    say 'dir D:\RS2DEV\src\*.iso'
elif [ -n "$rs2" ]; then
    say 'call D:\RS2DEV\src\BUILD.CMD userland64'
    say 'dir D:\RS2DEV\src\work\userland\64'
fi
say 'echo end'
[ ! -f work/disk.txt ] || cp work/disk.txt "$t/disk.save"
trap '[ ! -f $t/disk.save ] || cp $t/disk.save work/disk.txt' EXIT
printf '%s -> t.cmd\n' "$cmd" > work/disk.txt
cargo xtask disk --hpfs --size 4000 -o "$t/d.img" > /dev/null
# QEMU is stopped when the script has ended.
(
    while ! grep -aq '^end' work/serial.log 2> /dev/null || [ ! -f "$t/started" ]; do
        sleep 10
        touch "$t/started"
    done
    sleep 5
    pkill -f "qemu-system-x86_64 .*$t/d.img"
) > /dev/null 2>&1 &
: > work/serial.log
cargo xtask run --headless --timeout "${TIMEOUT:-3600}" --mem 8G --smp "${SMP:-4}" --hdd "$t/d.img" --virtio-cd "$iso" --init 'OS2\CMD.EXE /C D:\t.cmd' > "$t/log" 2>&1 || true
sed -n '/^begin/,$p' "$t/log" | tr '\r' '\n' | grep -av Building | tail -150
has() { grep -aq "$1" "$t/log" || { echo "run.sh: missing: $1" >&2; exit 1; }; }
has 'C: is not an HPFS or JFS drive'
[ "$(grep -ac 'The developer disk is installed' "$t/log")" = 2 ]
[ "$(grep -ac '^SET CARGO_HOME=D:.RS2DEV.cargo' "$t/log")" = 1 ]
[ "$(grep -ac '^LIBPATH=.*;D:.RS2DEV.sdk.bin' "$t/log")" = 1 ]
has 'D:/RS2DEV/sdk/bin/CLANG.EXE'
has 'Hello from C, 64-bit, on OS/2 20.45 (2 arguments)'
has 'MYLIB.DLL says 42'
has 'cpmhello selftest done: 0 failure'
has 'Hello C++ on rs2'
has 'exceptions work'
has 'Hello from Rust, 64-bit: 5050'
has 'PMHELLO.EXE\|pmhello.exe'
has 'Hello, world!'
has '^end'
if grep -aqE '^error' "$t/log"; then exit 1; fi
if [ "$rs2" = --iso ]; then
    cargo xtask diskget "$t/d.img" 'RS2DEV\src\rs2-live.iso' -o "$t/out"
    cargo xtask run --iso "$t/out/rs2-live.iso" --headless --timeout 150 --mem 4G --screenshot "$t/iso.png" > "$t/iso.log" 2>&1
    if grep -aqiE 'panic|exception' "$t/iso.log"; then echo "the ISO's boot"; exit 1; fi
    grep -aq 'smp: ' "$t/iso.log"
elif [ -n "$rs2" ]; then
    cargo xtask diskget "$t/d.img" 'RS2DEV\src\work\userland\64\STDTST64.EXE' 'RS2DEV\src\work\userland\64\LIBCXX64.DLL' -o "$t/out"
fi
