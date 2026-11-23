#!/bin/sh
# RSLLVM.DLL: rustc's LLVM and its C++ wrapper, behind their C interface
# (docs/SELF-HOSTING.md). rustc has rs2libc, the C++ code LIBC64.DLL: each
# keeps to its own heap.
#
#   rsllvm.sh EXPORTS OUT      EXPORTS: the functions rustc imports
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
dir=${RUSTC_DIR:-$root/work/rustc}
src=$dir/rustc-1.100.0-src
exports=$1
out=$2
cc=$root/userland64/c/cc64.sh
bin=$(ls -d "$(rustc --print sysroot)"/lib/rustlib/*/bin | head -1)
o=$dir/rsllvm
rm -rf "$o"
mkdir -p "$o"
flags="-std=c++17 -fno-rtti -fno-exceptions -ffunction-sections -fdata-sections -DNDEBUG -fvisibility=default -fvisibility-inlines-hidden
    -I$dir/llvm-rs2/include -I$src/src/llvm-project/llvm/include -I$src/compiler/rustc_llvm/llvm-wrapper -DLLVM_RUSTLLVM -w"
for c in IPO BITREADER BITWRITER LINKER ASMPARSER LTO COVERAGE INSTRUMENTATION X86; do flags="$flags -DLLVM_COMPONENT_$c"; done
for f in PassWrapper RustWrapper CoverageMappingWrapper SymbolWrapper Linker; do
    $cc -c $flags "$src/compiler/rustc_llvm/llvm-wrapper/$f.cpp" -o "$o/$f.o" &
done
wait
$cc -c $flags -fvisibility=hidden "$here/rsllvm.cpp" -o "$o/rsllvm.o"
# LLVM's C functions: their objects out of the archives, with the functions
# rustc imports made visible (LLVM is compiled with hidden symbols).
for l in $(cat "$dir/llvm-rs2/rustc-libs.txt"); do
    "$bin/llvm-nm" -A --defined-only "$l" 2> /dev/null | awk -v lib="$l" 'NR == FNR { want[$1] = 1; next } $NF in want { split($1, a, ":"); print lib, a[2] }' "$exports" -
done | sort -u > "$o/members"
n=0
while read -r lib m; do
    n=$((n + 1))
    (mkdir "$o/$n" && cd "$o/$n" && "$bin/llvm-ar" x "$lib" "$m" && mv "$m" "../c$n.o")
    rmdir "$o/$n"
    "$bin/llvm-objcopy" --set-symbols-visibility=/dev/null "$o/c$n.o" 2> /dev/null || true
    "$bin/llvm-objcopy" --wildcard $(sed 's/^/--set-symbol-visibility=/; s/$/=default/' "$exports") "$o/c$n.o"
done < "$o/members"
$cc --cxx --dll -o "$out" "$o"/*.o $(cat "$dir/llvm-rs2/rustc-libs.txt")
"$bin/llvm-nm" -D --defined-only "$out" | awk '{ print $NF }' | sort > "$o/have"
sort "$exports" | comm -23 - "$o/have" > "$o/missing"
[ -s "$o/missing" ] && { echo "rsllvm.sh: not in $out:" >&2; cat "$o/missing" >&2; }
ls -l "$out"
