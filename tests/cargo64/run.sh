#!/bin/sh
# cargo on rs2 (docs/SELF-HOSTING.md): builds, runs and tests a workspace
# there (a build script, a proc macro, path dependencies, crates/inifile).
# Needs userland64/rustc/build.sh, userland64/cargo/build.sh and clang's
# userland64/llvm/build.sh done.
#
# The toolchain and the sources are on an HPFS disk (drive E:) made here.
# Changes work/disk.txt for the run and puts it back.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
cd "$root"
t=$root/work/cargo64test
rm -rf "$t"
mkdir -p "$t"
userland64/rustc/sdk.sh "$t/sdk" > /dev/null
cp -R "$here/ws" "$t/ws"
cp -R crates/inifile "$t/ws/inifile"
cmd=$t/build.cmd
printf '@echo off\r\nE:\r\ncd \\ws\r\nset PATH=E:\\sdk\\bin;%%PATH%%\r\nset BEGINLIBPATH=E:\\sdk\\bin\r\nset TMPDIR=E:\\tmp\r\nset CARGO_HOME=E:\\cargo\r\n' > "$cmd"
say() { printf '%s\r\n' "$1" >> "$cmd"; }
say 'echo begin'
say 'cargo -V'
say 'cargo build --offline'
say 'cargo run --offline -- x y'
say 'cargo test --offline --workspace'
say 'echo // changed >> app\src\main.rs'
say 'echo second build'
say 'cargo run --offline --release -- z'
say 'cargo build --offline --manifest-path E:\ws\mac\Cargo.toml'
say 'echo not rust >> mac\src\lib.rs'
say 'cargo check --offline'
say 'cargo clean'
say 'echo end'
[ ! -f work/disk.txt ] || cp work/disk.txt "$t/disk.save"
trap '[ ! -f $t/disk.save ] || cp $t/disk.save work/disk.txt' EXIT
{
    cat "$t/sdk.txt"
    echo "$here/run.sh -> tmp\\keep"
    echo "$cmd -> build.cmd"
    (cd "$t" && find ws -type f) | while read -r f; do printf '%s -> %s\n' "$t/$f" "$(echo "$f" | tr / '\\')"; done
} > work/disk.txt
cargo xtask disk --hpfs --size 1000 -o "$t/d.img" > /dev/null
cargo xtask run --headless --timeout "${TIMEOUT:-420}" --mem 6G --hdd "$t/d.img" --init 'OS2\CMD.EXE /C E:\build.cmd' > "$t/log" 2>&1
sed -n '/^begin/,$p' "$t/log" | tr '\r' '\n'
has() { grep -aq "$1" "$t/log" || { echo "run.sh: missing: $1" >&2; exit 1; }; }
has 'app: 7 42 x86_64-unknown-linux-gnu \["x", "y"\]'
has 'app: 7 42 x86_64-unknown-linux-gnu \["z"\]'
has 'test tests::generated ... ok'
has 'test tests::panics - should panic ... ok'
has 'Finished `release` profile'
has 'could not compile `mac`'
has 'Removed '
grep -aq '^end' "$t/log"
! grep -aq 'test result: FAILED' "$t/log"
