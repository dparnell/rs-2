// DOSSET.COM: what a session's DOS settings show a program: INT 12h's
// conventional memory (DOS_RMSIZE), the DOS version (DOS_VERSION), the
// Ctrl+C check (DOS_BREAK) and the largest free block (48h with BX=FFFFh).
// Prints "DOSSET RM=nnn VER=maj.min BREAK=n FREE=nnnnn".
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
    mov dx, offset s_head
    mov ah, 9
    int 0x21
    int 0x12
    call16 dec
    mov dx, offset s_ver
    mov ah, 9
    int 0x21
    mov ah, 0x30
    int 0x21
    push ax
    xor ah, ah
    call16 dec
    mov dl, '.'
    mov ah, 2
    int 0x21
    pop ax
    mov al, ah
    xor ah, ah
    call16 dec
    mov dx, offset s_break
    mov ah, 9
    int 0x21
    mov ax, 0x3300
    int 0x21
    mov al, dl
    xor ah, ah
    call16 dec
    mov dx, offset s_free
    mov ah, 9
    int 0x21
    // Shrink to 4 KB first, then ask for everything.
    mov ah, 0x4a
    mov bx, 0x100
    int 0x21
    mov ah, 0x48
    mov bx, 0xffff
    int 0x21
    mov ax, bx
    call16 dec
    mov dx, offset s_end
    mov ah, 9
    int 0x21
    mov ax, 0x4c00
    int 0x21

// Print AX in decimal.
dec:
    xor cx, cx
    mov bx, 10
1:  xor dx, dx
    div bx
    push dx
    inc cx
    test ax, ax
    jnz 1b
2:  pop dx
    add dl, '0'
    mov ah, 2
    int 0x21
    loop 2b
    .byte 0xc3

s_head: .ascii "DOSSET RM=$"
s_ver: .ascii " VER=$"
s_break: .ascii " BREAK=$"
s_free: .ascii " FREE=$"
s_end: .ascii "\r\n$"
