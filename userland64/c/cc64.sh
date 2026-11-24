#!/bin/sh
# Build a 64-bit rs2 program or DLL from C or C++: tools/cc64, where its
# options are described. (Built by rustc alone, and kept in target/cc64, so
# that it runs inside any build.)
here=$(cd "$(dirname "$0")" && pwd)
CC64_ROOT=$(cd "$here/../.." && pwd)
export CC64_ROOT
src=$CC64_ROOT/tools/cc64/src/main.rs
exe=$CC64_ROOT/target/cc64/cc64
if [ ! -x "$exe" ] || [ "$src" -nt "$exe" ]; then
    mkdir -p "$CC64_ROOT/target/cc64" || exit
    env -u RUSTFLAGS -u CARGO_ENCODED_RUSTFLAGS -u CARGO_MANIFEST_DIR rustc --edition 2024 -O -C panic=abort --crate-name cc64 -o "$exe.$$" "$src" || exit
    mv "$exe.$$" "$exe" || exit
fi
exec "$exe" "$@"
