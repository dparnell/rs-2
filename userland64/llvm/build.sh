#!/bin/sh
# LLVM's lld for rs2 (docs/SELF-HOSTING.md): a 64-bit program, cross-built
# with cc64.sh through userland64/cmake.
#
#   build.sh [TARGET...]      (lld by default; results in work/llvm/rs2/bin,
#                              stripped: target/llvm64/LLD.EXE)
#
# Needs CMake, Ninja, clang and about 10 GB. LLVM's source comes from
# GitHub into $LLVM_DIR (work/llvm), where the host's table generators and
# the build go too.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
dir=${LLVM_DIR:-$root/work/llvm}
ver=21.1.8
tarsha=4633a23617fa31a3ea51242586ea7fb1da7140e426bd62fc164261fe036aa142
src=$dir/llvm-project-$ver.src
[ $# -gt 0 ] || set -- lld
mkdir -p "$dir"
# The source is unpacked again when the patch changes.
if command -v sha256sum > /dev/null; then sha=sha256sum; else sha="shasum -a 256"; fi
patch=$($sha < "$here/rs2.patch")
if [ "$(cat "$src/.rs2-patched" 2> /dev/null)" != "$patch" ]; then
    tar=$dir/llvm.tar.xz
    [ -f "$tar" ] || curl -fL -o "$tar" "https://github.com/llvm/llvm-project/releases/download/llvmorg-$ver/llvm-project-$ver.src.tar.xz"
    sum=$($sha < "$tar")
    [ "${sum%% *}" = "$tarsha" ] || { echo "build.sh: $tar does not have SHA-256 $tarsha" >&2; exit 1; }
    rm -rf "$src"
    (cd "$dir" && tar xf llvm.tar.xz llvm-project-$ver.src/llvm llvm-project-$ver.src/lld llvm-project-$ver.src/clang \
        llvm-project-$ver.src/cmake llvm-project-$ver.src/libunwind llvm-project-$ver.src/third-party)
    (cd "$src" && patch -p1 < "$here/rs2.patch" && echo "$patch" > .rs2-patched)
fi
common="-G Ninja -S $src/llvm -DCMAKE_BUILD_TYPE=Release -DLLVM_TARGETS_TO_BUILD=X86 -DLLVM_ENABLE_PROJECTS=clang;lld
    -DLLVM_INCLUDE_TESTS=OFF -DLLVM_INCLUDE_BENCHMARKS=OFF -DLLVM_INCLUDE_EXAMPLES=OFF -DLLVM_INCLUDE_DOCS=OFF
    -DLLVM_ENABLE_ZLIB=OFF -DLLVM_ENABLE_ZSTD=OFF -DLLVM_ENABLE_LIBXML2=OFF -DLLVM_APPEND_VC_REV=OFF"
[ -f "$dir/host/build.ninja" ] || cmake $common -B "$dir/host"
ninja -C "$dir/host" llvm-min-tblgen llvm-tblgen clang-tblgen
"$root/userland64/c/cc64.sh" --libcxx
triple=x86_64-unknown-rs2
[ -f "$dir/rs2/build.ninja" ] || cmake $common -B "$dir/rs2" -DCMAKE_TOOLCHAIN_FILE="$root/userland64/cmake/rs2.cmake" \
    -DCMAKE_BUILD_TYPE=MinSizeRel -DLLVM_HOST_TRIPLE=$triple -DLLVM_DEFAULT_TARGET_TRIPLE=$triple \
    -DLLVM_NATIVE_TOOL_DIR="$dir/host/bin" -DLLVM_TABLEGEN="$dir/host/bin/llvm-tblgen" -DCLANG_TABLEGEN="$dir/host/bin/clang-tblgen" \
    -DLLVM_INCLUDE_UTILS=OFF -DLLVM_BUILD_TOOLS=OFF -DLLVM_ENABLE_LIBEDIT=OFF -DLLVM_ENABLE_LIBPFM=OFF -DLLVM_ENABLE_PLUGINS=OFF \
    -DLLVM_ENABLE_BACKTRACES=OFF -DLLVM_ENABLE_CRASH_OVERRIDES=OFF -DLLVM_ENABLE_BINDINGS=OFF -DLLVM_ENABLE_WARNINGS=OFF \
    -DLLVM_FORCE_VC_REVISION=llvmorg-$ver -DLLVM_FORCE_VC_REPOSITORY=llvm -DCLANG_ENABLE_STATIC_ANALYZER=OFF -DCLANG_PLUGIN_SUPPORT=OFF
ninja -C "$dir/rs2" "$@"
strip=$(ls -d "$(rustc --print sysroot)"/lib/rustlib/*/bin/llvm-objcopy | head -1)
mkdir -p "$root/target/llvm64"
for f in "$dir"/rs2/bin/*.EXE; do
    [ -L "$f" ] || "$strip" --strip-all "$f" "$root/target/llvm64/$(basename "$f" | tr '[:lower:]' '[:upper:]')"
done
ls -l "$root/target/llvm64"
