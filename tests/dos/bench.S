// BENCH.COM: a CPU-bound loop (about 100 million instructions of
// arithmetic, memory and string work) for measuring the emulator.
.intel_syntax noprefix
.code16
.text
.globl _start
.org 0x100
_start:
    mov bp, 400
1:  mov cx, 25000
    mov si, offset buf
2:  mov ax, [si]
    add ax, cx
    xor dx, dx
    imul ax, ax, 3
    mov [si], ax
    shl ax, 1
    adc dx, 0
    inc si
    and si, 0xff
    add si, offset buf
    loop 2b
    dec bp
    jnz 1b
    mov ah, 9
    mov dx, offset done
    int 0x21
    mov ax, 0x4c00
    int 0x21
done: .ascii "bench done\r\n$"
buf: .fill 512, 1, 0
