#!/bin/sh
# Assemble the CPU test payloads in crates/x86emu/tests/asm and, with
# --oracle, run them on QEMU to record the expected output (*.expected).
# Needs clang (any with the x86 target) and llvm-objcopy (rustup's
# llvm-tools has one); QEMU's qemu-system-i386 for --oracle.
set -e
here=$(cd "$(dirname "$0")/../../crates/x86emu/tests/asm" && pwd)
objcopy=$(ls ~/.rustup/toolchains/*/lib/rustlib/*/bin/llvm-objcopy 2>/dev/null | head -1)
[ -n "$objcopy" ] || objcopy=llvm-objcopy
tmp=${TMPDIR:-/tmp}/rs2-cputest.$$
mkdir -p "$tmp"
asm() {
    base=${2:-0}
    clang -target i386-unknown-none-elf -c "$here/$1.S" -o "$tmp/$1.o"
    ld.lld -m elf_i386 --image-base=$base -Ttext=$base -e _start "$tmp/$1.o" -o "$tmp/$1.elf" 2>/dev/null ||
        "$(dirname "$objcopy")/rust-lld" -flavor gnu -m elf_i386 --image-base=$base -Ttext=$base -e _start "$tmp/$1.o" -o "$tmp/$1.elf"
    "$objcopy" -O binary "$tmp/$1.elf" "$here/$1.bin"
}
asm boot
for t in alu pm; do
    [ -f "$here/$t.S" ] || continue
    case $t in pm) asm $t 0x10000 ;; *) asm $t ;; esac
    if [ "$1" = "--oracle" ]; then
        cat "$here/boot.bin" "$here/$t.bin" > "$tmp/$t.img"
        # Pad to whole sectors (and beyond what the boot sector reads).
        dd if=/dev/zero bs=1024 count=160 >> "$tmp/$t.img" 2>/dev/null
        rm -f "$tmp/$t.out"
        perl -e 'alarm shift; exec @ARGV' 300 qemu-system-i386 -display none -drive file="$tmp/$t.img",format=raw,if=ide \
            -debugcon file:"$tmp/$t.out" -device isa-debug-exit,iobase=0xf4,iosize=4 \
            -cpu 486 -m 16 || true
        cp "$tmp/$t.out" "$here/$t.expected"
        echo "$t: $(wc -c < "$here/$t.expected") bytes"
    fi
done
rm -rf "$tmp"
