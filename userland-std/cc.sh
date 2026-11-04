#!/bin/sh
# C compiler for C code in userland-std crates (ring's): clang for i686
# Linux, freestanding, with rs2libc's headers (userland-std/include).
dir=$(cd "$(dirname "$0")" && pwd)
exec clang --target=i686-unknown-linux-gnu -ffreestanding -fno-stack-protector -isystem "$dir/include" "$@"
