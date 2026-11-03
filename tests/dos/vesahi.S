// VESAHI.COM: VBE direct colour modes. Finds 640x480 with 16 bits per
// pixel (5:6:5) and with 32 bits (8:8:8) in the controller's mode list
// through 4F01h's mode information, sets each and draws a colour ramp
// through the banked window (red across, green down, blue rising in
// bands), waiting for a key after each; then text mode again.
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
    mov ax, 0x4f00
    mov di, offset info
    mov dword ptr [info], 0x32454256    // "VBE2"
    int 0x10
    cmp ax, 0x004f
    jne fail
    cmp word ptr [info + 4], 0x0200
    jb fail
    // 640x480x16, then 640x480x32.
    mov byte ptr [want], 16
    call16 find
    jc fail
    call16 draw16
    mov byte ptr [want], 32
    call16 find
    jc fail
    call16 draw32
    mov ax, 3
    int 0x10
    mov ah, 9
    mov dx, offset ok
    int 0x21
    mov ax, 0x4c00
    int 0x21
fail:
    mov ax, 3
    int 0x10
    mov ah, 9
    mov dx, offset bad
    int 0x21
    mov ax, 0x4c01
    int 0x21

// The mode list (far pointer at info+14): a 640x480 mode with [want]
// bits per pixel and direct colour (model 6); sets it. CF on failure.
find:
    push ds
    pop es
    push ds
    lds si, [info + 14]
    mov bx, ds
    pop ds
1:  push ds
    mov ds, bx
    lodsw
    pop ds
    cmp ax, 0xffff
    je 9f
    mov cx, ax
    mov [mode], ax
    push bx
    push si
    mov ax, 0x4f01
    mov di, offset minfo
    int 0x10
    pop si
    pop bx
    cmp ax, 0x004f
    jne 1b
    cmp word ptr [minfo + 0x12], 640
    jne 1b
    cmp word ptr [minfo + 0x14], 480
    jne 1b
    mov al, [want]
    cmp [minfo + 0x19], al
    jne 1b
    cmp byte ptr [minfo + 0x1b], 6
    jne 1b
    mov ax, 0x4f02
    mov bx, [mode]
    int 0x10
    cmp ax, 0x004f
    jne 9f
    clc
    .byte 0xc3
9:  stc
    .byte 0xc3

// The next bank when DI wrapped.
nextbank:
    test di, di
    jnz 1f
    push ax
    push bx
    push dx
    inc word ptr [bank]
    mov ax, 0x4f05
    xor bx, bx
    mov dx, [bank]
    int 0x10
    pop dx
    pop bx
    pop ax
1:  .byte 0xc3

startdraw:
    mov word ptr [bank], 0
    mov ax, 0x4f05
    xor bx, bx
    xor dx, dx
    int 0x10
    push 0xa000
    pop es
    xor di, di
    xor dx, dx
    .byte 0xc3

// 5:6:5: red = x / 20, green = y / 8, blue = (x + y) / 35 & 31.
draw16:
    call16 startdraw
1:  xor cx, cx
2:  mov ax, cx
    push dx
    xor dx, dx
    mov bx, 20
    div bx
    pop dx
    shl ax, 11
    mov bx, dx
    shr bx, 3
    shl bx, 5
    or ax, bx
    mov bx, cx
    add bx, dx
    push ax
    push dx
    mov ax, bx
    xor dx, dx
    mov bx, 35
    div bx
    and ax, 31
    mov bx, ax
    pop dx
    pop ax
    or ax, bx
    stosw
    call16 nextbank
    inc cx
    cmp cx, 640
    jb 2b
    inc dx
    cmp dx, 480
    jb 1b
    xor ax, ax
    int 0x16
    .byte 0xc3

// 8:8:8: red = x * 2 / 5, green = y / 2, blue = 255 - x * 2 / 5.
draw32:
    call16 startdraw
1:  xor cx, cx
2:  mov ax, cx
    shl ax, 1
    push dx
    xor dx, dx
    mov bx, 5
    div bx
    pop dx
    mov bl, al
    mov al, 255
    sub al, bl
    stosb               // blue
    mov ax, dx
    shr ax, 1
    stosb               // green
    mov al, bl
    stosb               // red
    xor al, al
    stosb
    call16 nextbank
    inc cx
    cmp cx, 640
    jb 2b
    inc dx
    cmp dx, 480
    jb 1b
    xor ax, ax
    int 0x16
    .byte 0xc3

ok: .ascii "VESAHI OK: 640x480 in 16 and 32 bits per pixel\r\n$"
bad: .ascii "VESAHI FAILED\r\n$"
want: .byte 0
mode: .word 0
bank: .word 0
.balign 4
info: .fill 512, 1, 0
minfo: .fill 256, 1, 0
