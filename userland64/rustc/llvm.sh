#!/bin/sh
# rustc's own LLVM for rs2, as static libraries (docs/SELF-HOSTING.md).
# Used by build.sh, which has unpacked and patched the source.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
dir=${RUSTC_DIR:-$root/work/rustc}
src=$dir/rustc-1.100.0-src/src/llvm-project
common="-G Ninja -S $src/llvm -DCMAKE_BUILD_TYPE=Release -DLLVM_TARGETS_TO_BUILD=X86
    -DLLVM_INCLUDE_TESTS=OFF -DLLVM_INCLUDE_BENCHMARKS=OFF -DLLVM_INCLUDE_EXAMPLES=OFF -DLLVM_INCLUDE_DOCS=OFF
    -DLLVM_ENABLE_ZLIB=OFF -DLLVM_ENABLE_ZSTD=OFF -DLLVM_ENABLE_LIBXML2=OFF -DLLVM_APPEND_VC_REV=OFF
    -DLLVM_FORCE_VC_REVISION=rust-1.100.0 -DLLVM_FORCE_VC_REPOSITORY=rust"
[ -f "$dir/llvm-host/build.ninja" ] || cmake $common -B "$dir/llvm-host"
ninja -C "$dir/llvm-host" llvm-min-tblgen llvm-tblgen llvm-config
"$root/userland64/c/cc64.sh" --libcxx
triple=x86_64-unknown-none-elf
[ -f "$dir/llvm-rs2/build.ninja" ] || cmake $common -B "$dir/llvm-rs2" -DCMAKE_TOOLCHAIN_FILE="$root/userland64/cmake/rs2.cmake" \
    -DCMAKE_BUILD_TYPE=MinSizeRel -DLLVM_HOST_TRIPLE=$triple -DLLVM_DEFAULT_TARGET_TRIPLE=x86_64-unknown-linux-gnu \
    -DLLVM_NATIVE_TOOL_DIR="$dir/llvm-host/bin" -DLLVM_TABLEGEN="$dir/llvm-host/bin/llvm-tblgen" \
    -DLLVM_INCLUDE_UTILS=OFF -DLLVM_BUILD_TOOLS=OFF -DLLVM_ENABLE_LIBEDIT=OFF -DLLVM_ENABLE_LIBPFM=OFF -DLLVM_ENABLE_PLUGINS=OFF \
    -DLLVM_ENABLE_BACKTRACES=OFF -DLLVM_ENABLE_CRASH_OVERRIDES=OFF -DLLVM_ENABLE_BINDINGS=OFF -DLLVM_ENABLE_WARNINGS=OFF
# The libraries rustc needs (llvm-config names what it looks for and does
# not find in the host's build), and those the host's tools have.
libs=$( (cd "$dir/llvm-host/lib" && ls libLLVM*.a | grep -v TableGen | sed 's|^|lib/|')
    "$dir/llvm-host/bin/llvm-config" --link-static --libnames ipo bitreader bitwriter linker asmparser lto coverage instrumentation x86 2>&1 |
    grep -o 'libLLVM[A-Za-z0-9]*\.a' | sort -u | sed 's|^|lib/|')
ninja -C "$dir/llvm-rs2" $libs
for l in $libs; do echo "$dir/llvm-rs2/$l"; done > "$dir/llvm-rs2/rustc-libs.txt"
