#!/bin/sh
# The native toolchain for rs2 with rustc added to clang's (sdk.sh of
# userland64/llvm):
#
#   sdk.sh [DIR]              (target/sdk64 by default)
#
#   DIR/bin      RUSTC.EXE, RS2-LD.EXE, RSLLVM.DLL
#   DIR/lib/rs2  what RS2-LD.EXE links into programs and DLLs (rs2libc as
#                libc.a, start-up code, link64.ld)
#   DIR/lib/rustlib   the standard library (rustup's, as it is)
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
std=$root/userland-std
out=${1:-$root/target/sdk64}
t=x86_64-unknown-linux-gnu
"$root/userland64/llvm/sdk.sh" "$out" > /dev/null
bin=$(ls -d "$(rustc --print sysroot)"/lib/rustlib/*/bin | head -1)
(cd "$std" && cargo build -q --release --target $t -p rs2libc -p rs2ld)
"$bin/llvm-objcopy" --strip-all "$root/target/rustc64/RUSTC.EXE" "$out/bin/RUSTC.EXE"
"$bin/llvm-objcopy" --strip-all "$std/target/$t/release/rs2-ld" "$out/bin/RS2-LD.EXE"
"$bin/llvm-objcopy" --strip-all "$root/target/rustc64/RSLLVM.DLL" "$out/bin/RSLLVM.DLL"
# rs2libc and what it uses as libc.a (std asks for -lc).
r=$out/lib/rs2
tmp=$(mktemp -d "${TMPDIR:-/tmp}/rs2libc.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$r"
cp "$std"/syslib/*.a "$r/"
rm "$r/libc.a"
for c in rs2libc libc libm dlmalloc unwinding gimli cfg_if; do
    f=$(find "$std/target/$t/release" -name "lib$c-*.rlib" | xargs ls -t | head -1)
    mkdir "$tmp/$c"
    (cd "$tmp/$c" && "$bin/llvm-ar" x "$f" && rm -f lib.rmeta)
done
"$bin/llvm-ar" rcs "$r/libc.a" $(find "$tmp" -type f -name '*.o')
cp "$std/link64.ld" "$r/"
for d in "" dll; do
    rustc --edition 2024 --target $t -O -C panic=abort -C relocation-model=pic ${d:+--cfg dll} --check-cfg 'cfg(dll)' \
        --emit=obj -o "$r/crt64${d:+d}.o" "$std/crt64.rs"
done
# The standard library, without std's own Linux libraries.
s=$(rustc --print sysroot)/lib/rustlib/$t/lib
mkdir -p "$out/lib/rustlib/$t/lib"
cp "$s"/*.rlib "$s"/*.rmeta "$out/lib/rustlib/$t/lib/"
(cd "$out" && find . -type f | sed 's|^\./||' | sort) | while read -r f; do
    printf '%s -> sdk\\%s\n' "$out/$f" "$(echo "$f" | tr / '\\')"
done > "$out.txt"
du -sh "$out"
