// MOUSE.COM: the INT 33h driver. Resets it, shows the pointer and
// writes "X=xxxx Y=yyyy B=b" (hex) on the top line as the mouse moves,
// until a key is pressed.
.intel_syntax noprefix
.code16
.macro call16 dest
    .byte 0xe8
    .word \dest - . - 2
.endm
.text
.globl _start
.org 0x100
_start:
    xor ax, ax
    int 0x33
    cmp ax, 0xffff
    je 1f
    mov ah, 9
    mov dx, offset nomouse
    int 0x21
    mov ax, 0x4c01
    int 0x21
1:  mov ax, 1
    int 0x33
2:  mov ax, 3
    int 0x33
    push bx
    push dx
    push cx
    mov ah, 2
    xor bh, bh
    xor dx, dx
    int 0x10
    mov si, offset sx
    call16 puts
    pop ax
    call16 hex4
    mov si, offset sy
    call16 puts
    pop ax
    call16 hex4
    mov si, offset sb
    call16 puts
    pop ax
    call16 hex4
    mov ah, 1
    int 0x16
    jz 2b
    xor ax, ax
    int 0x16
    mov ax, 2
    int 0x33
    mov ax, 0x4c00
    int 0x21
puts:
    lodsb
    test al, al
    jz 3f
    mov ah, 0x0e
    xor bx, bx
    int 0x10
    jmp puts
3:  .byte 0xc3
hex4:
    mov cx, 4
4:  rol ax, 4
    push ax
    and al, 15
    add al, '0'
    cmp al, '9'
    jbe 5f
    add al, 7
5:  mov ah, 0x0e
    xor bx, bx
    int 0x10
    pop ax
    loop 4b
    .byte 0xc3
sx: .asciz "X="
sy: .asciz " Y="
sb: .asciz " B="
nomouse: .ascii "No mouse driver.\r\n$"
