// GFX.COM: graphics modes. Mode 13h (all 256 colours in a 16x16 grid,
// drawn into A000h), then mode 12h (16 bands drawn with VGA write mode
// 2), each labelled through the BIOS teletype and held until a key is
// pressed; then text mode and "GFX OK".
.intel_syntax noprefix
.code16
.text
.macro call16 dest
    .byte 0xe8
    .word \dest - . - 2
.endm
.globl _start
.org 0x100
_start:
    mov ax, 0x13
    int 0x10
    push 0xa000
    pop es
    xor di, di
    xor dx, dx
1:  xor cx, cx
2:  // colour = (y / 13) * 16 + x / 20
    mov ax, dx
    mov bl, 13
    div bl
    mov bh, al
    shl bh, 4
    mov ax, cx
    mov bl, 20
    div bl
    add al, bh
    stosb
    inc cx
    cmp cx, 320
    jb 2b
    inc dx
    cmp dx, 200
    jb 1b
    mov si, offset m13
    mov dx, 0x1700
    call16 label
    xor ax, ax
    int 0x16
    // Mode 12h: 16 bands of 40 pixels (5 bytes) by write mode 2.
    mov ax, 0x12
    int 0x10
    mov dx, 0x3ce
    mov ax, 0x0205
    out dx, ax
    xor di, di
    mov bp, 480
3:  xor bl, bl
4:  mov cx, 5
5:  mov al, es:[di]
    mov es:[di], bl
    inc di
    loop 5b
    inc bl
    cmp bl, 16
    jb 4b
    dec bp
    jnz 3b
    mov dx, 0x3ce
    mov ax, 0x0005
    out dx, ax
    mov si, offset m12
    mov dx, 0x1c00
    call16 label
    xor ax, ax
    int 0x16
    mov ax, 3
    int 0x10
    mov ah, 9
    mov dx, offset done
    int 0x21
    mov ax, 0x4c00
    int 0x21

// Text at row DH, column DL in white.
label:
    mov ah, 2
    xor bh, bh
    int 0x10
6:  lodsb
    test al, al
    jz 7f
    mov ah, 0x0e
    mov bx, 0x000f
    int 0x10
    jmp 6b
7:  .byte 0xc3

m13: .asciz "Mode 13h: 256 colours. Press a key."
m12: .asciz "Mode 12h: 640x480, 16 colours. Press a key."
done: .ascii "GFX OK\r\n$"
