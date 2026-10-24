// VESA.COM: VBE 2.0. Checks the controller information ("VESA"), sets
// mode 101h (640x480, 256 colours) and draws a diagonal colour pattern
// through the banked window at A000h (switching banks with 4F05h), then
// waits for a key and goes back to text mode.
.intel_syntax noprefix
.code16
.text
.globl _start
.org 0x100
_start:
    mov ax, 0x4f00
    mov di, offset info
    int 0x10
    cmp ax, 0x004f
    jne fail
    cmp dword ptr [info], 0x41534556
    jne fail
    mov ax, 0x4f02
    mov bx, 0x101
    int 0x10
    cmp ax, 0x004f
    jne fail
    push 0xa000
    pop es
    xor bp, bp              // bank
    xor di, di              // offset in the window
    xor dx, dx              // y
1:  xor cx, cx              // x
2:  mov ax, cx
    add ax, dx
    shr ax, 1
    stosb
    test di, di
    jnz 3f
    // The window is full: the next bank.
    inc bp
    push dx
    mov ax, 0x4f05
    xor bx, bx
    mov dx, bp
    int 0x10
    pop dx
3:  inc cx
    cmp cx, 640
    jb 2b
    inc dx
    cmp dx, 480
    jb 1b
    xor ax, ax
    int 0x16
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
ok: .ascii "VESA OK\r\n$"
bad: .ascii "VESA FAILED\r\n$"
info: .fill 512, 1, 0
