// HELLO.COM: text output through DOS and the BIOS, the command tail,
// the environment, a file round trip, the exit code.
.intel_syntax noprefix
.code16
.text
.globl _start
.org 0x100
_start:
    mov ah, 9
    mov dx, offset msg
    int 0x21
    // The command tail, as given.
    mov si, 0x81
    xor cx, cx
    mov cl, [0x80]
1:  jcxz 2f
    lodsb
    mov dl, al
    mov ah, 2
    int 0x21
    dec cx
    jmp 1b
2:  mov ah, 9
    mov dx, offset crlf
    int 0x21
    // BIOS teletype.
    mov si, offset bmsg
3:  lodsb
    test al, al
    jz 4f
    mov ah, 0x0e
    mov bx, 7
    int 0x10
    jmp 3b
4:  // Create, write, reopen, read back a file.
    mov ah, 0x3c
    xor cx, cx
    mov dx, offset fname
    int 0x21
    jc fail
    mov bx, ax
    mov ah, 0x40
    mov cx, 5
    mov dx, offset data
    int 0x21
    jc fail
    mov ah, 0x3e
    int 0x21
    mov ax, 0x3d00
    mov dx, offset fname
    int 0x21
    jc fail
    mov bx, ax
    mov ah, 0x3f
    mov cx, 16
    mov dx, offset buf
    int 0x21
    jc fail
    cmp ax, 5
    jne fail
    mov ah, 0x3e
    int 0x21
    mov ah, 0x41
    mov dx, offset fname
    int 0x21
    mov ah, 9
    mov dx, offset ok
    int 0x21
    mov ax, 0x4c07
    int 0x21
fail:
    mov ah, 9
    mov dx, offset bad
    int 0x21
    mov ax, 0x4c01
    int 0x21
msg:  .ascii "Hello from DOS!\r\nArgs:[$"
crlf: .ascii "]\r\n$"
bmsg: .asciz "BIOS teletype works.\r\n"
fname: .asciz "HELLO.TMP"
data: .ascii "12345"
ok:   .ascii "File I/O OK\r\n$"
bad:  .ascii "File I/O FAILED\r\n$"
buf:  .fill 16
