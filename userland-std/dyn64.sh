#!/bin/sh
# Programs that share a Rust DLL's std (docs/STD64.md):
#
#   dyn64.sh PACKAGE        (a program; built in target/dyn64)
#
# rustc takes std from a dylib the program uses only if std itself has no
# dylib: rustup's sysroot has a libstd.so (Linux's). This makes a sysroot
# without it, of links, and builds with it.
set -e
dir=$(cd "$(dirname "$0")" && pwd)
t=x86_64-unknown-linux-gnu
from=$(rustc --print sysroot)/lib/rustlib/$t/lib
sys=$dir/target/sysroot64
to=$sys/lib/rustlib/$t/lib
rm -rf "$sys"
mkdir -p "$to"
for f in "$from"/*.rlib "$from"/*.rmeta; do
    ln -s "$f" "$to/"
done
cd "$dir"
exec cargo rustc --release --target $t --target-dir target/dyn64 -p "$1" \
    --config "target.$t.rustflags=['--sysroot', '$sys']" -- -C prefer-dynamic
