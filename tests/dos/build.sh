#!/bin/sh
# Assemble the DOS test programs (*.S -> *.COM, or *.EXE for sources
# that build their own MZ header) into tests/dos/bin.
set -e
here=$(cd "$(dirname "$0")" && pwd)
objcopy=$(ls ~/.rustup/toolchains/*/lib/rustlib/*/bin/llvm-objcopy 2>/dev/null | head -1)
lld=$(ls ~/.rustup/toolchains/*/lib/rustlib/*/bin/rust-lld 2>/dev/null | head -1)
mkdir -p "$here/bin"
tmp=${TMPDIR:-/tmp}/rs2-dostest.$$
mkdir -p "$tmp"
for s in "$here"/*.S; do
    n=$(basename "$s" .S)
    ext=COM
    grep -q '^// FORMAT: EXE' "$s" && ext=EXE
    clang -target i386-unknown-none-elf -c "$s" -o "$tmp/$n.o"
    "$lld" -flavor gnu -m elf_i386 --image-base=0 -Ttext=0 -e _start "$tmp/$n.o" -o "$tmp/$n.elf"
    "$objcopy" -O binary "$tmp/$n.elf" "$tmp/$n.bin"
    up=$(echo "$n" | tr a-z A-Z)
    if [ $ext = COM ]; then
        # Linked at 0 with .org 100h: drop the PSP part.
        tail -c +257 "$tmp/$n.bin" > "$here/bin/$up.COM"
    else
        cp "$tmp/$n.bin" "$here/bin/$up.EXE"
    fi
done
rm -rf "$tmp"
