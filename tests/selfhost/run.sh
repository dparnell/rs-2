#!/bin/sh
# rs2's kernel and boot loader built on rs2 (docs/SELF-HOSTING.md), and
# rs2 booted with them. Needs userland64/rustc/build.sh,
# userland64/cargo/build.sh and clang's userland64/llvm/build.sh done.
#
#   run.sh             (about an hour in an emulator)
#
# The toolchain and the committed sources are on an HPFS disk (drive E:)
# made here. Changes work/disk.txt for the run and puts it back.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
cd "$root"
t=$root/work/selfhosttest
rm -rf "$t"
mkdir -p "$t/rs2"
userland64/rustc/sdk.sh "$t/sdk" > /dev/null
# The sources, with the dependencies in vendor.
git archive HEAD Cargo.toml Cargo.lock .cargo boot kernel crates tools xtask | tar xf - -C "$t/rs2"
(cd "$t/rs2" && cargo vendor --locked vendor > /dev/null 2>&1)
printf '\n[source.crates-io]\nreplace-with = "vendored"\n[source.vendored]\ndirectory = "vendor"\n' >> "$t/rs2/.cargo/config.toml"
cmd=$t/build.cmd
printf '@echo off\r\nE:\r\ncd \\rs2\r\nset PATH=E:\\sdk\\bin;%%PATH%%\r\nset BEGINLIBPATH=E:\\sdk\\bin\r\nset TMPDIR=E:\\tmp\r\nset CARGO_HOME=E:\\cargo\r\nset CARGO_INCREMENTAL=0\r\n' > "$cmd"
say() { printf '%s\r\n' "$1" >> "$cmd"; }
say 'echo begin'
say 'cargo build --offline --locked -p rs2-boot --target x86_64-unknown-uefi --profile checked'
say 'cargo build --offline --locked -p rs2-kernel --target x86_64-unknown-none --profile checked'
say 'dir target\x86_64-unknown-uefi\checked\*.efi'
say 'dir target\x86_64-unknown-none\checked\rs2-kernel'
say 'echo end'
[ ! -f work/disk.txt ] || cp work/disk.txt "$t/disk.save"
trap '[ ! -f $t/disk.save ] || cp $t/disk.save work/disk.txt' EXIT
{
    cat "$t/sdk.txt"
    echo "$here/run.sh -> tmp\\keep"
    echo "$cmd -> build.cmd"
    (cd "$t" && find rs2 -type f) | while read -r f; do printf '%s -> %s\n' "$t/$f" "$(echo "$f" | tr / '\\')"; done
} > work/disk.txt
cargo xtask disk --hpfs --size 1500 -o "$t/d.img" > /dev/null
cargo xtask run --headless --timeout "${TIMEOUT:-3000}" --mem 8G --smp "${SMP:-4}" --hdd "$t/d.img" --init 'OS2\CMD.EXE /C E:\build.cmd' > "$t/log" 2>&1
sed -n '/^begin/,$p' "$t/log" | tr '\r' '\n' | grep -av Building
cargo xtask diskget "$t/d.img" 'rs2\target\x86_64-unknown-uefi\checked\rs2-boot.efi' 'rs2\target\x86_64-unknown-none\checked\rs2-kernel' -o "$t/out"
ls -l "$t/out"
