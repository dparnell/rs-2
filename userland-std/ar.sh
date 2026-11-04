#!/bin/sh
# Archiver for C code in userland-std crates: the llvm-ar of rustup's
# llvm-tools (GNU archive format, which rust-lld reads), else llvm-ar on
# PATH.
ar="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/^host: //p')/bin/llvm-ar"
[ -x "$ar" ] || ar=llvm-ar
exec "$ar" "$@"
